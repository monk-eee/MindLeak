//! Deterministic symbol and call-graph extraction (heuristic, zero-token).
//!
//! Pattern-based extraction covering the common languages. It finds symbol
//! **definitions** and, within each callable definition's body, **call sites**
//! that reference another symbol defined in the same file — emitting in-file
//! `calls` edges. Structured behind [`extract`] so a Tree-sitter backend can
//! replace the heuristics later (for cross-file / scope-accurate resolution)
//! without touching callers.

use std::collections::HashSet;

use regex::Regex;

/// A source symbol definition discovered in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub line: usize,
}

/// An in-file call: `caller` (a symbol defined in the file) references `callee`
/// (another symbol defined in the same file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub caller: String,
    pub callee: String,
}

/// The result of analysing one file.
#[derive(Debug, Clone, Default)]
pub struct Extraction {
    pub symbols: Vec<Symbol>,
    pub calls: Vec<Call>,
}

/// Internal definition record with the byte offset of its name.
struct Def {
    name: String,
    kind: String,
    line: usize,
    name_offset: usize,
}

/// Kinds that can contain calls (i.e. have a body worth scanning).
const CALLABLE_KINDS: &[&str] = &["fn", "function", "arrow", "def", "func", "method"];

/// Per-language definition patterns and whether the language is brace-scoped
/// (`true`) or indentation-scoped (`false`, e.g. Python).
fn language_config(ext: &str) -> (&'static [(&'static str, &'static str)], bool) {
    match ext {
        "rs" => (
            &[
                (
                    r"(?m)^\s*(?:pub\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+([A-Za-z_]\w*)",
                    "fn",
                ),
                (r"(?m)^\s*(?:pub\s+)?struct\s+([A-Za-z_]\w*)", "struct"),
                (r"(?m)^\s*(?:pub\s+)?enum\s+([A-Za-z_]\w*)", "enum"),
                (r"(?m)^\s*(?:pub\s+)?trait\s+([A-Za-z_]\w*)", "trait"),
            ],
            true,
        ),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => (
            &[
                (
                    r"(?m)^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s+([A-Za-z_$][\w$]*)",
                    "function",
                ),
                (
                    r"(?m)^\s*(?:export\s+)?(?:abstract\s+)?class\s+([A-Za-z_$][\w$]*)",
                    "class",
                ),
                (
                    r"(?m)^\s*(?:export\s+)?interface\s+([A-Za-z_$][\w$]*)",
                    "interface",
                ),
                (
                    r"(?m)^\s*(?:export\s+)?const\s+([A-Za-z_$][\w$]*)\s*=\s*(?:async\s*)?\(",
                    "arrow",
                ),
            ],
            true,
        ),
        "py" => (
            &[
                (r"(?m)^\s*def\s+([A-Za-z_]\w*)", "def"),
                (r"(?m)^\s*class\s+([A-Za-z_]\w*)", "class"),
            ],
            false,
        ),
        "cs" => (
            &[(
                r"(?m)(?:class|interface|struct|record|enum)\s+([A-Za-z_]\w*)",
                "type",
            )],
            true,
        ),
        "go" => (
            &[
                (r"(?m)^\s*func\s+(?:\([^)]*\)\s*)?([A-Za-z_]\w*)", "func"),
                (r"(?m)^\s*type\s+([A-Za-z_]\w*)", "type"),
            ],
            true,
        ),
        "java" | "kt" => (
            &[(r"(?m)(?:class|interface|enum)\s+([A-Za-z_]\w*)", "type")],
            true,
        ),
        _ => (&[], true),
    }
}

fn find_defs(content: &str, patterns: &[(&str, &str)]) -> Vec<Def> {
    let mut defs = Vec::new();
    for (pattern, kind) in patterns {
        let re = match Regex::new(pattern) {
            Ok(re) => re,
            Err(_) => continue,
        };
        for caps in re.captures_iter(content) {
            if let Some(m) = caps.get(1) {
                let line = 1 + content[..m.start()].bytes().filter(|&b| b == b'\n').count();
                defs.push(Def {
                    name: m.as_str().to_string(),
                    kind: (*kind).to_string(),
                    line,
                    name_offset: m.start(),
                });
            }
        }
    }
    defs
}

/// Compute the `[start, end)` byte span of a definition's body.
fn body_span(content: &str, def: &Def, brace_lang: bool) -> Option<(usize, usize)> {
    if brace_lang {
        let open = content[def.name_offset..].find('{')? + def.name_offset;
        let mut depth = 0usize;
        for (i, &b) in content.as_bytes().iter().enumerate().skip(open) {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some((open + 1, i));
                    }
                }
                _ => {}
            }
        }
        None
    } else {
        // Indentation-scoped (Python): body = following lines indented deeper
        // than the definition line.
        let line_start = content[..def.name_offset]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        let def_indent = content[line_start..]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .count();
        let body_start = content[def.name_offset..]
            .find('\n')
            .map(|p| p + def.name_offset + 1)?;
        let mut pos = body_start;
        let mut end = content.len();
        while pos < content.len() {
            let line_end = content[pos..]
                .find('\n')
                .map(|p| p + pos)
                .unwrap_or(content.len());
            let line = &content[pos..line_end];
            if !line.trim_start().is_empty() {
                let indent = line.chars().take_while(|c| *c == ' ' || *c == '\t').count();
                if indent <= def_indent {
                    end = pos;
                    break;
                }
            }
            pos = line_end + 1;
        }
        Some((body_start, end))
    }
}

/// Analyse a file into symbol definitions and in-file call edges.
pub fn extract(path: &str, content: &str) -> Extraction {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let (patterns, brace_lang) = language_config(&ext);
    let defs = find_defs(content, patterns);

    let symbols: Vec<Symbol> = defs
        .iter()
        .map(|d| Symbol {
            name: d.name.clone(),
            kind: d.kind.clone(),
            line: d.line,
        })
        .collect();

    let callable_names: HashSet<&str> = defs
        .iter()
        .filter(|d| CALLABLE_KINDS.contains(&d.kind.as_str()))
        .map(|d| d.name.as_str())
        .collect();

    // Body spans for callable definitions (for innermost-caller attribution).
    let bodies: Vec<(&str, usize, usize)> = defs
        .iter()
        .filter(|d| CALLABLE_KINDS.contains(&d.kind.as_str()))
        .filter_map(|d| body_span(content, d, brace_lang).map(|(s, e)| (d.name.as_str(), s, e)))
        .collect();

    let def_offsets: HashSet<usize> = defs.iter().map(|d| d.name_offset).collect();

    let mut calls = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    if !callable_names.is_empty() {
        let call_re = Regex::new(r"([A-Za-z_$][\w$]*)\s*\(").unwrap();
        for caps in call_re.captures_iter(content) {
            let m = caps.get(1).unwrap();
            let callee = m.as_str();
            // Skip definition signatures (they look like calls) and unknown callees.
            if def_offsets.contains(&m.start()) || !callable_names.contains(callee) {
                continue;
            }
            let off = m.start();
            let caller = bodies
                .iter()
                .filter(|(_, s, e)| off >= *s && off < *e)
                .min_by_key(|(_, s, e)| e - s)
                .map(|(n, _, _)| *n);
            if let Some(caller) = caller {
                if caller != callee && seen.insert((caller.to_string(), callee.to_string())) {
                    calls.push(Call {
                        caller: caller.to_string(),
                        callee: callee.to_string(),
                    });
                }
            }
        }
    }

    Extraction { symbols, calls }
}

/// Extract just the symbol definitions from `content` (convenience wrapper).
pub fn extract_symbols(path: &str, content: &str) -> Vec<Symbol> {
    extract(path, content).symbols
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_rust_symbols() {
        let src = "pub fn validate_session() {}\nstruct Session {}\n";
        let syms = extract_symbols("src/auth.rs", src);
        assert!(syms
            .iter()
            .any(|s| s.name == "validate_session" && s.kind == "fn"));
        assert!(syms
            .iter()
            .any(|s| s.name == "Session" && s.kind == "struct"));
    }

    #[test]
    fn extracts_typescript_symbols() {
        let src = "export function validateSession() {}\nclass Auth {}\nexport const run = async () => {}\n";
        let syms = extract_symbols("src/auth.ts", src);
        assert!(syms.iter().any(|s| s.name == "validateSession"));
        assert!(syms.iter().any(|s| s.name == "Auth" && s.kind == "class"));
        assert!(syms.iter().any(|s| s.name == "run" && s.kind == "arrow"));
    }

    #[test]
    fn line_numbers_are_one_based() {
        let src = "\n\ndef helper():\n    pass\n";
        let syms = extract_symbols("x.py", src);
        assert_eq!(syms[0].line, 3);
    }

    #[test]
    fn unknown_extension_yields_nothing() {
        assert!(extract_symbols("data.bin", "garbage").is_empty());
    }

    #[test]
    fn extracts_rust_call_edges() {
        let src = "fn a() {\n    b();\n    helper();\n}\nfn b() {}\nfn helper() {}\n";
        let calls = extract("src/x.rs", src).calls;
        assert!(calls.contains(&Call {
            caller: "a".into(),
            callee: "b".into()
        }));
        assert!(calls.contains(&Call {
            caller: "a".into(),
            callee: "helper".into()
        }));
    }

    #[test]
    fn extracts_python_call_edges() {
        let src = "def a():\n    b()\n\ndef b():\n    pass\n";
        let calls = extract("m.py", src).calls;
        assert_eq!(
            calls,
            vec![Call {
                caller: "a".into(),
                callee: "b".into()
            }]
        );
    }

    #[test]
    fn ignores_self_recursion_and_unknown_callees() {
        let src = "fn a() {\n    a();\n    unknown_fn();\n}\n";
        let calls = extract("src/x.rs", src).calls;
        assert!(calls.is_empty()); // self-loop skipped; unknown_fn not defined here
    }

    #[test]
    fn attributes_call_to_innermost_definition() {
        // A call inside `b` must be attributed to b, not the earlier `a`.
        let src = "fn a() {}\nfn b() {\n    a();\n}\n";
        let calls = extract("src/x.rs", src).calls;
        assert_eq!(
            calls,
            vec![Call {
                caller: "b".into(),
                callee: "a".into()
            }]
        );
    }
}
