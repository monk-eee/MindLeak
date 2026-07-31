//! Whether an identifier at a position is shadowed by a nearer binding.

use super::binding::{
    binding_names, contains_bare_require_call, parameter_binding_names, variable_declarators,
};
use super::callable::callable_for_body;
use super::nav::next_non_newline;
use super::scope::{block_path, is_path_prefix, nearest_function_path};
use super::Token;

pub(crate) fn is_identifier_shadowed(tokens: &[Token], at: usize, name: &str) -> bool {
    is_identifier_shadowed_except(tokens, at, name, None)
}

pub(crate) fn is_identifier_shadowed_except(
    tokens: &[Token],
    at: usize,
    name: &str,
    allowed_binding_offset: Option<usize>,
) -> bool {
    let call_path = block_path(tokens, at);
    if call_path
        .iter()
        .filter_map(|body| callable_for_body(tokens, *body))
        .any(|callable| {
            parameter_binding_names(&tokens[callable.parameter_start + 1..callable.parameter_end])
                .iter()
                .any(|binding| binding == name)
        })
    {
        return true;
    }

    for index in 0..tokens.len() {
        let declaration = tokens[index].identifier();
        if !matches!(
            declaration,
            Some("const" | "let" | "var" | "function" | "class")
        ) || !declaration_binds_name(tokens, index, name)
        {
            continue;
        }
        if allowed_binding_offset
            .is_some_and(|offset| declaration_binding_at_offset(tokens, index, name, offset))
        {
            continue;
        }
        let declaration_path = if declaration == Some("var") {
            nearest_function_path(tokens, index)
        } else {
            block_path(tokens, index)
        };
        if is_path_prefix(&declaration_path, &call_path) {
            return true;
        }
    }
    false
}

fn declaration_binding_at_offset(
    tokens: &[Token],
    declaration: usize,
    name: &str,
    offset: usize,
) -> bool {
    match tokens[declaration].identifier() {
        Some("function" | "class") => {
            next_non_newline(tokens, declaration + 1).is_some_and(|index| {
                tokens[index].identifier() == Some(name) && tokens[index].start == offset
            })
        }
        Some("const" | "let" | "var") => {
            next_non_newline(tokens, declaration + 1).is_some_and(|index| {
                tokens[index].identifier() == Some(name) && tokens[index].start == offset
            })
        }
        _ => false,
    }
}

fn declaration_binds_name(tokens: &[Token], declaration: usize, name: &str) -> bool {
    match tokens[declaration].identifier() {
        Some("function" | "class") => {
            next_non_newline(tokens, declaration + 1).and_then(|index| tokens[index].identifier())
                == Some(name)
        }
        Some("const" | "let" | "var") => {
            variable_declarators(tokens, declaration + 1)
                .iter()
                .any(|declarator| {
                    binding_names(declarator)
                        .iter()
                        .any(|binding| binding == name)
                        && !contains_bare_require_call(declarator)
                })
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::javascript::tokenize;

    #[test]
    fn shadowing_handles_parameters_all_declarators_and_var_scope() {
        let source = "function scoped(require): void { require('x'); } require('real');";
        let tokens = tokenize(source);
        let calls: Vec<usize> = tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| token.identifier() == Some("require"))
            .map(|(index, _)| index)
            .collect();
        assert!(is_identifier_shadowed(&tokens, calls[1], "require"));
        assert!(!is_identifier_shadowed(&tokens, calls[2], "require"));

        let source =
            "function consumer() { dependency(); const first = value,\n dependency = first; }";
        let tokens = tokenize(source);
        let call = tokens
            .iter()
            .position(|token| token.identifier() == Some("dependency"))
            .unwrap();
        assert!(is_identifier_shadowed(&tokens, call, "dependency"));

        let source =
            "function consumer() { dependency(); try { var dependency = local; } finally {} }";
        let tokens = tokenize(source);
        let call = tokens
            .iter()
            .position(|token| token.identifier() == Some("dependency"))
            .unwrap();
        assert!(is_identifier_shadowed(&tokens, call, "dependency"));
    }

    #[test]
    fn typed_function_and_arrow_parameters_shadow_require() {
        let source = "function scoped(require): () => void { require('ghost'); }";
        let tokens = tokenize(source);
        let calls: Vec<usize> = tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| token.identifier() == Some("require"))
            .map(|(index, _)| index)
            .collect();
        assert!(is_identifier_shadowed(&tokens, calls[1], "require"));

        let source = "const scoped = (require): void => { require('ghost'); };";
        let tokens = tokenize(source);
        let calls: Vec<usize> = tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| token.identifier() == Some("require"))
            .map(|(index, _)| index)
            .collect();
        assert!(is_identifier_shadowed(&tokens, calls[1], "require"));
    }
}
