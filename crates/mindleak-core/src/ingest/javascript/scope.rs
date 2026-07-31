//! Lexical block scoping: which nested blocks enclose a token position.

use super::callable::callable_for_body;
use super::Token;

pub(super) fn block_path(tokens: &[Token], end: usize) -> Vec<usize> {
    let mut path = Vec::new();
    for (index, token) in tokens.iter().enumerate().take(end) {
        if token.is_punctuation('{') {
            path.push(index);
        } else if token.is_punctuation('}') {
            path.pop();
        }
    }
    path
}

pub(super) fn is_path_prefix(prefix: &[usize], path: &[usize]) -> bool {
    prefix.len() <= path.len() && prefix.iter().zip(path).all(|(left, right)| left == right)
}

pub(super) fn nearest_function_path(tokens: &[Token], at: usize) -> Vec<usize> {
    let path = block_path(tokens, at);
    match path
        .iter()
        .rposition(|body| callable_for_body(tokens, *body).is_some())
    {
        Some(index) => path[..=index].to_vec(),
        None => Vec::new(),
    }
}
