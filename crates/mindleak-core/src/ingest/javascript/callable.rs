//! Function and arrow callable boundaries: parameters and body span.

use super::nav::{matching_close, next_non_newline, next_punctuation, previous_non_newline};
use super::Token;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Callable {
    pub body_open: usize,
    pub body_close: usize,
    pub parameter_start: usize,
    pub parameter_end: usize,
}

pub(crate) fn callable_for_definition(
    tokens: &[Token],
    definition: usize,
    kind: &str,
) -> Option<Callable> {
    let (parameter_start, parameter_end, body_open) = if kind == "arrow" {
        let assignment = next_punctuation(tokens, definition + 1, '=')?;
        let parameter_start = next_punctuation(tokens, assignment + 1, '(')?;
        let parameter_end = matching_close(tokens, parameter_start, '(', ')')?;
        let arrow = outer_arrow_after(tokens, parameter_end)?;
        let body_open = next_punctuation(tokens, arrow + 2, '{')?;
        (parameter_start, parameter_end, body_open)
    } else {
        let parameter_start = next_punctuation(tokens, definition + 1, '(')?;
        let parameter_end = matching_close(tokens, parameter_start, '(', ')')?;
        let body_open = body_after_signature(tokens, parameter_end)?;
        (parameter_start, parameter_end, body_open)
    };
    let body_close = matching_close(tokens, body_open, '{', '}')?;
    Some(Callable {
        body_open,
        body_close,
        parameter_start,
        parameter_end,
    })
}

pub(super) fn callable_for_body(tokens: &[Token], body_open: usize) -> Option<Callable> {
    for index in (0..body_open).rev() {
        let Some(name) = tokens[index].identifier() else {
            continue;
        };
        let kind = if name == "function" {
            continue;
        } else if previous_non_newline(tokens, index)
            .is_some_and(|previous| tokens[previous].identifier() == Some("function"))
        {
            "function"
        } else {
            "arrow"
        };
        if let Some(callable) = callable_for_definition(tokens, index, kind) {
            if callable.body_open == body_open {
                return Some(callable);
            }
        }
    }
    None
}

fn outer_arrow_after(tokens: &[Token], start: usize) -> Option<usize> {
    let mut round = 0usize;
    let mut square = 0usize;
    let mut curly = 0usize;
    for index in start + 1..tokens.len().saturating_sub(1) {
        if tokens[index].is_punctuation('(') {
            round += 1;
        } else if tokens[index].is_punctuation(')') {
            round = round.saturating_sub(1);
        } else if tokens[index].is_punctuation('[') {
            square += 1;
        } else if tokens[index].is_punctuation(']') {
            square = square.saturating_sub(1);
        } else if tokens[index].is_punctuation('{') {
            curly += 1;
        } else if tokens[index].is_punctuation('}') {
            curly = curly.saturating_sub(1);
        } else if round == 0
            && square == 0
            && curly == 0
            && tokens[index].is_punctuation('=')
            && tokens[index + 1].is_punctuation('>')
        {
            return Some(index);
        }
    }
    None
}

fn body_after_signature(tokens: &[Token], parameter_end: usize) -> Option<usize> {
    let mut cursor = next_non_newline(tokens, parameter_end + 1)?;
    if tokens[cursor].is_punctuation('{') {
        return Some(cursor);
    }
    if !tokens[cursor].is_punctuation(':') {
        return None;
    }
    cursor += 1;
    let mut round = 0usize;
    let mut square = 0usize;
    while cursor < tokens.len() {
        if tokens[cursor].is_punctuation('(') {
            round += 1;
        } else if tokens[cursor].is_punctuation(')') {
            round = round.saturating_sub(1);
        } else if tokens[cursor].is_punctuation('[') {
            square += 1;
        } else if tokens[cursor].is_punctuation(']') {
            square = square.saturating_sub(1);
        } else if round == 0 && square == 0 && tokens[cursor].is_punctuation('{') {
            let close = matching_close(tokens, cursor, '{', '}')?;
            let after = next_non_newline(tokens, close + 1);
            if let Some(after) = after {
                if tokens[after].is_punctuation('{') {
                    return Some(after);
                }
                if tokens[after].is_punctuation('|') || tokens[after].is_punctuation('&') {
                    cursor = close + 1;
                    continue;
                }
            }
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::javascript::tokenize;

    #[test]
    fn callable_parser_skips_nested_defaults_and_return_type_braces() {
        let source = "const consumer = (callback = () => {}) => { dependency(); };";
        let tokens = tokenize(source);
        let definition = tokens
            .iter()
            .position(|token| token.identifier() == Some("consumer"))
            .unwrap();
        let callable = callable_for_definition(&tokens, definition, "arrow").unwrap();
        assert!(tokens[callable.body_open].start > source.find("=> {}").unwrap());

        let source = "function consumer({ flag }): () => { value: string } { dependency(); }";
        let tokens = tokenize(source);
        let definition = tokens
            .iter()
            .position(|token| token.identifier() == Some("consumer"))
            .unwrap();
        let callable = callable_for_definition(&tokens, definition, "function").unwrap();
        assert!(tokens[callable.body_open].start > source.find("value: string").unwrap());
        assert!(tokens[callable.parameter_start + 1..callable.parameter_end]
            .iter()
            .any(|token| token.identifier() == Some("flag")));
    }
}
