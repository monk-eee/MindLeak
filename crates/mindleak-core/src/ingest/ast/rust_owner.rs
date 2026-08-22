//! Qualify Rust method definitions by their enclosing `impl` owner.

use regex::Regex;
use std::sync::OnceLock;

use super::body::matching_brace_end;
use super::types::Def;

struct OwnerSpan {
    name: String,
    start: usize,
    end: usize,
}

pub(super) fn qualify_rust_methods(content: &str, defs: &mut [Def]) {
    let owners = rust_impl_owner_spans(content);
    for def in defs.iter_mut().filter(|definition| definition.kind == "fn") {
        let owner = owners
            .iter()
            .filter(|owner| def.name_offset >= owner.start && def.name_offset < owner.end)
            .min_by_key(|owner| owner.end - owner.start);
        if let Some(owner) = owner {
            def.owner = Some(owner.name.clone());
            def.qualified_name = format!("{}::{}", owner.name, def.name);
        }
    }
}

fn rust_impl_owner_spans(content: &str) -> Vec<OwnerSpan> {
    rust_impl_re()
        .captures_iter(content)
        .filter_map(|capture| {
            let complete = capture.get(0)?;
            let header = capture.get(1)?.as_str();
            let open = complete.end().checked_sub(1)?;
            let end = matching_brace_end(content, open)?;
            Some(OwnerSpan {
                name: rust_impl_identity(header)?,
                start: open + 1,
                end,
            })
        })
        .collect()
}

fn rust_impl_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?ms)^[ \t]*(?:unsafe[ \t]+)?impl\b(.*?)\{").expect("valid Rust impl regex")
    })
}

fn rust_impl_identity(header: &str) -> Option<String> {
    let header = header.split_whitespace().collect::<Vec<_>>().join(" ");
    let header = strip_leading_generics(header.trim());
    let header = header.split(" where ").next()?.trim();
    if let Some((trait_name, target)) = header.rsplit_once(" for ") {
        Some(format!(
            "{}::{}",
            rust_type_name(target)?,
            rust_type_name(trait_name)?
        ))
    } else {
        rust_type_name(header)
    }
}

fn strip_leading_generics(value: &str) -> &str {
    if !value.starts_with('<') {
        return value;
    }
    let mut depth = 0usize;
    for (index, character) in value.char_indices() {
        match character {
            '<' => depth += 1,
            '>' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return value[index + character.len_utf8()..].trim_start();
                }
            }
            _ => {}
        }
    }
    value
}

fn rust_type_name(value: &str) -> Option<String> {
    let without_generics = value.split('<').next()?.trim();
    let path = without_generics
        .split_whitespace()
        .last()?
        .trim_matches(|character: char| {
            !character.is_ascii_alphanumeric() && character != '_' && character != ':'
        });
    path.rsplit("::")
        .find(|segment| !segment.is_empty())
        .map(str::to_string)
}
