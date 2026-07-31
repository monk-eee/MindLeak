//! Variable declarators and the names a binding pattern introduces.

use super::nav::{next_non_newline, previous_non_newline, split_top_level, top_level_position};
use super::Token;

pub(crate) fn variable_declarator_at(
    tokens: &[Token],
    declaration: usize,
    at: usize,
) -> Option<&[Token]> {
    variable_declarators_with_ranges(tokens, declaration + 1)
        .into_iter()
        .find(|(start, end)| at >= *start && at < *end)
        .map(|(start, end)| &tokens[start..end])
}

pub(super) fn variable_declarators(tokens: &[Token], start: usize) -> Vec<&[Token]> {
    variable_declarators_with_ranges(tokens, start)
        .into_iter()
        .map(|(segment_start, segment_end)| &tokens[segment_start..segment_end])
        .collect()
}

fn variable_declarators_with_ranges(tokens: &[Token], start: usize) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut segment_start = start;
    let mut round = 0usize;
    let mut square = 0usize;
    let mut curly = 0usize;
    let mut index = start;
    while index <= tokens.len() {
        let at_end = index == tokens.len();
        let newline_ends = !at_end
            && tokens[index].is_newline()
            && match previous_non_newline(tokens, index) {
                Some(previous) => !tokens[previous].is_punctuation(','),
                None => true,
            };
        let statement_end = !at_end
            && round == 0
            && square == 0
            && curly == 0
            && (tokens[index].is_punctuation(';')
                || newline_ends
                || tokens[index].is_punctuation('}'));
        let comma =
            !at_end && round == 0 && square == 0 && curly == 0 && tokens[index].is_punctuation(',');
        if at_end || statement_end || comma {
            ranges.push((segment_start, index));
            if at_end || statement_end {
                break;
            }
            segment_start = index + 1;
        } else if tokens[index].is_punctuation('(') {
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
        }
        index += 1;
    }
    ranges
}

pub(super) fn binding_names(tokens: &[Token]) -> Vec<String> {
    let end = tokens
        .iter()
        .position(|token| token.is_punctuation('='))
        .unwrap_or(tokens.len());
    let binding = &tokens[..end];
    let mut bindings = Vec::new();
    if !binding
        .first()
        .is_some_and(|token| token.is_punctuation('{') || token.is_punctuation('['))
    {
        if let Some(first) = binding.iter().find_map(Token::identifier) {
            bindings.push(first.to_string());
            return bindings;
        }
    }
    for (index, token) in binding.iter().enumerate() {
        let Some(identifier) = token.identifier() else {
            continue;
        };
        if !binding
            .get(index + 1)
            .is_some_and(|next| next.is_punctuation(':'))
        {
            bindings.push(identifier.to_string());
        }
    }
    bindings
}

pub(super) fn contains_bare_require_call(tokens: &[Token]) -> bool {
    tokens.iter().enumerate().any(|(index, token)| {
        token.identifier() == Some("require")
            && match previous_non_newline(tokens, index) {
                Some(previous) => !tokens[previous].is_punctuation('.'),
                None => true,
            }
            && next_non_newline(tokens, index + 1)
                .is_some_and(|next| tokens[next].is_punctuation('('))
    })
}

pub(super) fn parameter_binding_names(tokens: &[Token]) -> Vec<String> {
    split_top_level(tokens, ',')
        .into_iter()
        .flat_map(binding_pattern_names)
        .collect()
}

fn binding_pattern_names(tokens: &[Token]) -> Vec<String> {
    let end = top_level_position(tokens, '=')
        .or_else(|| top_level_position(tokens, ':'))
        .unwrap_or(tokens.len());
    let pattern = &tokens[..end];
    if pattern
        .first()
        .is_some_and(|token| token.is_punctuation('{'))
    {
        let inner = &pattern[1..pattern.len().saturating_sub(1)];
        return split_top_level(inner, ',')
            .into_iter()
            .flat_map(|property| {
                if let Some(colon) = top_level_position(property, ':') {
                    binding_pattern_names(&property[colon + 1..])
                } else {
                    property
                        .iter()
                        .find_map(Token::identifier)
                        .map(|name| vec![name.to_string()])
                        .unwrap_or_default()
                }
            })
            .collect();
    }
    if pattern
        .first()
        .is_some_and(|token| token.is_punctuation('['))
    {
        let inner = &pattern[1..pattern.len().saturating_sub(1)];
        return split_top_level(inner, ',')
            .into_iter()
            .flat_map(binding_pattern_names)
            .collect();
    }
    pattern
        .iter()
        .find_map(Token::identifier)
        .map(|name| vec![name.to_string()])
        .unwrap_or_default()
}
