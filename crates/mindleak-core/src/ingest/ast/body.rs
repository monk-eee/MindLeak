//! Compute the byte span of a definition's body.

use super::types::Def;
use crate::ingest::javascript::{callable_for_definition, tokenize};

pub(super) fn matching_brace_end(content: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, byte) in content.as_bytes().iter().enumerate().skip(open) {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Compute the `[start, end)` byte span of a definition's body.
pub(super) fn body_span(
    content: &str,
    def: &Def,
    brace_lang: bool,
    javascript: bool,
) -> Option<(usize, usize)> {
    if javascript {
        let tokens = tokenize(content);
        let definition = tokens.iter().position(|token| {
            token.start >= def.name_offset && token.identifier() == Some(def.name.as_str())
        })?;
        let callable = callable_for_definition(&tokens, definition, &def.kind)?;
        Some((
            tokens[callable.body_open].end,
            tokens[callable.body_close].start,
        ))
    } else if brace_lang {
        let open = content[def.name_offset..].find('{')? + def.name_offset;
        matching_brace_end(content, open).map(|end| (open + 1, end))
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
