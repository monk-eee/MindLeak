//! Token-stream navigation shared by the JavaScript submodules.

use super::Token;

pub(super) fn split_top_level(tokens: &[Token], separator: char) -> Vec<&[Token]> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut round = 0usize;
    let mut square = 0usize;
    let mut curly = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        if round == 0 && square == 0 && curly == 0 && token.is_punctuation(separator) {
            result.push(&tokens[start..index]);
            start = index + 1;
        } else if token.is_punctuation('(') {
            round += 1;
        } else if token.is_punctuation(')') {
            round = round.saturating_sub(1);
        } else if token.is_punctuation('[') {
            square += 1;
        } else if token.is_punctuation(']') {
            square = square.saturating_sub(1);
        } else if token.is_punctuation('{') {
            curly += 1;
        } else if token.is_punctuation('}') {
            curly = curly.saturating_sub(1);
        }
    }
    result.push(&tokens[start..]);
    result
}

pub(super) fn top_level_position(tokens: &[Token], punctuation: char) -> Option<usize> {
    split_top_level(tokens, punctuation)
        .first()
        .filter(|first| first.len() < tokens.len())
        .map(|first| first.len())
}

pub(super) fn matching_close(
    tokens: &[Token],
    open: usize,
    left: char,
    right: char,
) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if token.is_punctuation(left) {
            depth += 1;
        } else if token.is_punctuation(right) {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

pub(super) fn next_punctuation(tokens: &[Token], start: usize, punctuation: char) -> Option<usize> {
    (start..tokens.len()).find(|index| tokens[*index].is_punctuation(punctuation))
}

pub(super) fn next_non_newline(tokens: &[Token], start: usize) -> Option<usize> {
    (start..tokens.len()).find(|index| !tokens[*index].is_newline())
}

pub(super) fn previous_non_newline(tokens: &[Token], index: usize) -> Option<usize> {
    (0..index)
        .rev()
        .find(|previous| !tokens[*previous].is_newline())
}
