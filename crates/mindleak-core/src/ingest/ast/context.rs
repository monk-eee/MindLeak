//! The text worth embedding for a symbol: its declaration line, plus any doc
//! comment written immediately above it.

/// Doc-comment lines kept above a declaration. Enough for an intent sentence,
/// short enough that a long licence header never becomes the symbol's meaning.
const MAX_DOC_LINES: usize = 8;
/// Characters kept. Embedding quality falls off well before this, and an
/// unbounded body would let one enormous signature dominate a batch.
pub(super) const MAX_CONTEXT_CHARS: usize = 400;

/// The text worth embedding for a symbol: its declaration line, plus any doc
/// comment written immediately above it.
///
/// Symbol nodes stored `path:line` and nothing else, so the only thing an
/// embedding could see was the symbol's *name*. That made terse implementation
/// names (`effective_weight`, `prune`, `recall`) embed as near-noise while long
/// descriptive test names embedded richly — so `recall` systematically returned
/// the tests instead of the code under test. Measured: querying the literal
/// identifier `effective_weight` did not return `effective_weight`.
///
/// Deterministic and zero-token: this is text already parsed off disk, never a
/// model call (invariant 1).
///
/// `line` is 1-based, matching [`Symbol::line`].
pub fn symbol_context(content: &str, line: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let Some(index) = line.checked_sub(1).filter(|index| *index < lines.len()) else {
        return String::new();
    };

    let mut kept: Vec<&str> = Vec::new();
    let mut cursor = index;
    while cursor > 0 && kept.len() < MAX_DOC_LINES {
        let candidate = lines[cursor - 1].trim();
        if is_attribute(candidate) {
            // Attributes sit between the doc comment and the declaration; step
            // over them rather than stopping, or `#[inline]` would hide the doc.
            cursor -= 1;
            continue;
        }
        if !is_doc_comment(candidate) {
            break;
        }
        kept.push(candidate);
        cursor -= 1;
    }
    kept.reverse();
    kept.push(lines[index].trim());

    let joined = kept.join("\n");
    match joined.char_indices().nth(MAX_CONTEXT_CHARS) {
        Some((boundary, _)) => joined[..boundary].to_string(),
        None => joined,
    }
}

/// A comment line in any of the languages the extractor covers.
fn is_doc_comment(line: &str) -> bool {
    line.starts_with("//")
        || line.starts_with("/*")
        || line.starts_with('*')
        || line.starts_with("\"\"\"")
        // `#` is a comment in Python, Ruby, and shell -- but `#[` and `#!` are
        // Rust attributes, which `is_attribute` handles separately.
        || (line.starts_with('#') && !line.starts_with("#[") && !line.starts_with("#!"))
}

/// A Rust attribute, which separates a doc comment from its declaration.
fn is_attribute(line: &str) -> bool {
    line.starts_with("#[") || line.starts_with("#![")
}
