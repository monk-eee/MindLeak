//! Per-language definition patterns and whether the language is brace-scoped.

/// Per-language definition patterns and whether the language is brace-scoped
/// (`true`) or indentation-scoped (`false`, e.g. Python).
pub(super) fn language_config(ext: &str) -> (&'static [(&'static str, &'static str)], bool) {
    match ext {
        "rs" => (
            &[
                (
                    r#"(?m)^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:(?:const|async|unsafe|extern(?:\s+"[^"]*"))\s+)*fn\s+([A-Za-z_]\w*)"#,
                    "fn",
                ),
                (
                    r"(?m)^\s*(?:pub(?:\s*\([^)]*\))?\s+)?struct\s+([A-Za-z_]\w*)",
                    "struct",
                ),
                (
                    r"(?m)^\s*(?:pub(?:\s*\([^)]*\))?\s+)?enum\s+([A-Za-z_]\w*)",
                    "enum",
                ),
                (
                    r"(?m)^\s*(?:pub(?:\s*\([^)]*\))?\s+)?trait\s+([A-Za-z_]\w*)",
                    "trait",
                ),
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
