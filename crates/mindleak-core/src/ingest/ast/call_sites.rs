//! Heuristic call-site discovery: regex-based for most languages, token-based
//! for JavaScript/TypeScript.

use regex::Regex;
use std::sync::OnceLock;

use crate::ingest::javascript::{next_non_newline, previous_non_newline, Token};

pub(super) fn regex_call_sites(content: &str) -> Vec<(String, usize, Option<usize>)> {
    call_site_re()
        .captures_iter(content)
        .filter_map(|captures| {
            captures
                .get(1)
                .map(|matched| (matched.as_str().to_string(), matched.start(), None))
        })
        .collect()
}

/// An identifier immediately followed by `(` — a heuristic call site. Compiled
/// once rather than on every extraction.
fn call_site_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"([A-Za-z_$][\w$]*)\s*\(").expect("valid call regex"))
}

pub(super) fn javascript_call_sites(tokens: &[Token]) -> Vec<(String, usize, Option<usize>)> {
    let mut sites = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let Some(name) = token.identifier() else {
            continue;
        };
        let Some(next) = next_non_newline(tokens, index + 1) else {
            continue;
        };
        if !tokens[next].is_punctuation('(') {
            continue;
        }
        if previous_non_newline(tokens, index)
            .is_some_and(|previous| tokens[previous].is_punctuation('.'))
        {
            continue;
        }
        sites.push((name.to_string(), token.start, Some(index)));
    }
    sites
}
