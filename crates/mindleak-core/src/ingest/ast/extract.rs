//! Analyse a file into symbol definitions and in-file call edges.

use std::collections::{HashMap, HashSet};

use crate::ingest::javascript::{
    is_identifier_shadowed, is_identifier_shadowed_except,
    mask_non_code as mask_javascript_non_code, tokenize,
};
use crate::ingest::source_mask::mask_non_code as mask_source_non_code;

use super::body::body_span;
use super::call_sites::{javascript_call_sites, regex_call_sites};
use super::defs::find_defs;
use super::language::language_config;
use super::rust_owner::qualify_rust_methods;
use super::types::{Call, CallReference, Def, Extraction, Symbol};

/// Kinds that can contain calls (i.e. have a body worth scanning).
const CALLABLE_KINDS: &[&str] = &["fn", "function", "arrow", "def", "func", "method"];

/// Analyse a file into symbol definitions and in-file call edges.
pub fn extract(path: &str, content: &str) -> Extraction {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let javascript = matches!(ext.as_str(), "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs");
    let (patterns, brace_lang) = language_config(&ext);
    let masked = if javascript {
        mask_javascript_non_code(content)
    } else {
        mask_source_non_code(&ext, content)
    };
    let mut defs = find_defs(&masked, patterns);
    if ext == "rs" {
        qualify_rust_methods(&masked, &mut defs);
    }

    let symbols: Vec<Symbol> = defs
        .iter()
        .map(|d| Symbol {
            name: d.name.clone(),
            qualified_name: d.qualified_name.clone(),
            kind: d.kind.clone(),
            line: d.line,
        })
        .collect();

    let mut callable_defs: HashMap<&str, Vec<&Def>> = HashMap::new();
    for definition in defs
        .iter()
        .filter(|definition| CALLABLE_KINDS.contains(&definition.kind.as_str()))
    {
        callable_defs
            .entry(definition.name.as_str())
            .or_default()
            .push(definition);
    }

    // Body spans for callable definitions (for innermost-caller attribution).
    let bodies: Vec<(&Def, usize, usize)> = defs
        .iter()
        .filter(|d| CALLABLE_KINDS.contains(&d.kind.as_str()))
        .filter_map(|d| {
            let body_source = if javascript { content } else { &masked };
            body_span(body_source, d, brace_lang, javascript).map(|(start, end)| (d, start, end))
        })
        .collect();

    let def_offsets: HashSet<usize> = defs.iter().map(|d| d.name_offset).collect();

    let mut calls = Vec::new();
    let mut call_references = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut references_seen: HashSet<(String, String)> = HashSet::new();
    if !bodies.is_empty() {
        let javascript_tokens = javascript.then(|| tokenize(content));
        let call_sites = if let Some(tokens) = &javascript_tokens {
            javascript_call_sites(tokens)
        } else {
            regex_call_sites(&masked)
        };
        for (callee, offset, token_index) in call_sites {
            // Definition signatures look like calls to the regex.
            if def_offsets.contains(&offset) {
                continue;
            }
            let caller = bodies
                .iter()
                .filter(|(_, start, end)| offset >= *start && offset < *end)
                .min_by_key(|(_, start, end)| end - start)
                .map(|(definition, _, _)| *definition);
            if let Some(caller) = caller {
                let local_candidates = callable_defs
                    .get(callee.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let local_definition = resolve_local_definition(local_candidates, caller);
                let shadowed = match (&javascript_tokens, token_index) {
                    (Some(tokens), Some(index)) => {
                        if let Some(definition) = local_definition {
                            is_identifier_shadowed_except(
                                tokens,
                                index,
                                &callee,
                                Some(definition.name_offset),
                            )
                        } else {
                            is_identifier_shadowed(tokens, index, &callee)
                        }
                    }
                    _ => false,
                };
                if shadowed {
                    continue;
                }
                if let Some(local_definition) = local_definition {
                    if caller.qualified_name == local_definition.qualified_name {
                        continue;
                    }
                    let edge = (
                        caller.qualified_name.clone(),
                        local_definition.qualified_name.clone(),
                    );
                    if seen.insert(edge.clone()) {
                        calls.push(Call {
                            caller: edge.0,
                            callee: edge.1,
                        });
                    }
                } else if local_candidates.is_empty()
                    && references_seen.insert((caller.qualified_name.clone(), callee.clone()))
                {
                    call_references.push(CallReference {
                        caller: caller.qualified_name.clone(),
                        callee,
                    });
                }
            }
        }
    }

    Extraction {
        symbols,
        calls,
        call_references,
    }
}

fn resolve_local_definition<'a>(candidates: &[&'a Def], caller: &Def) -> Option<&'a Def> {
    if candidates.len() == 1 {
        return candidates.first().copied();
    }
    let mut same_owner = candidates
        .iter()
        .copied()
        .filter(|candidate| candidate.owner == caller.owner);
    let resolved = same_owner.next()?;
    same_owner.next().is_none().then_some(resolved)
}

/// Extract just the symbol definitions from `content` (convenience wrapper).
pub fn extract_symbols(path: &str, content: &str) -> Vec<Symbol> {
    extract(path, content).symbols
}
