//! Finding raw definitions from per-language patterns, with cached compiled
//! regexes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use regex::Regex;

use super::types::Def;

pub(super) fn find_defs(content: &str, patterns: &[(&str, &str)]) -> Vec<Def> {
    let mut defs = Vec::new();
    for (pattern, kind) in patterns {
        let re = match compiled_pattern(pattern) {
            Some(re) => re,
            None => continue,
        };
        for caps in re.captures_iter(content) {
            if let Some(m) = caps.get(1) {
                let line = 1 + content[..m.start()].bytes().filter(|&b| b == b'\n').count();
                defs.push(Def {
                    name: m.as_str().to_string(),
                    qualified_name: m.as_str().to_string(),
                    owner: None,
                    kind: (*kind).to_string(),
                    line,
                    name_offset: m.start(),
                });
            }
        }
    }
    defs
}

/// Compile each language's constant definition pattern once and reuse it across
/// files, keyed by the pattern text. `find_defs` previously recompiled every
/// pattern on every call; a pattern that fails to compile is still skipped
/// (`None`) and never cached, exactly as before.
pub(super) fn compiled_pattern(pattern: &str) -> Option<Arc<Regex>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Regex>>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(re) = cache.get(pattern) {
        return Some(Arc::clone(re));
    }
    let re = Arc::new(Regex::new(pattern).ok()?);
    cache.insert(pattern.to_string(), Arc::clone(&re));
    Some(re)
}
