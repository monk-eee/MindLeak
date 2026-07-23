//! Small deterministic helpers: stable short hashes and slug ids.

use sha2::{Digest, Sha256};

/// Short stable hash used to build deterministic node ids.
pub(crate) fn short_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    digest.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// Turn a title into a stable, readable slug for goal ids. Falls back to a hash
/// when the title has no alphanumeric content.
pub(crate) fn slugify(input: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in input.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !out.is_empty() && !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    // Truncate first, then trim: trimming before the length cap can leave a
    // trailing separator when the 48-char boundary lands on a dash.
    let truncated: String = out.chars().take(48).collect();
    let slug = truncated.trim_matches('-');
    if slug.is_empty() {
        short_hash(input)
    } else {
        slug.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_is_kebab_and_bounded() {
        assert_eq!(slugify("Zero-Token Write Path!"), "zero-token-write-path");
        assert_eq!(slugify("  spaced  out  "), "spaced-out");
    }

    #[test]
    fn slugify_falls_back_to_hash_when_empty() {
        let s = slugify("!!!");
        assert!(!s.is_empty());
        assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // Regression: trimming '-' before the 48-char cap could leave a trailing
    // dash when the truncation boundary landed on a separator, producing goal
    // ids like "goal:...-". Truncating first, then trimming, prevents it.
    #[test]
    fn slugify_never_ends_with_dash_after_truncation() {
        let slug = slugify(&"a ".repeat(30));
        assert!(slug.starts_with('a'));
        assert!(!slug.ends_with('-'), "slug had a trailing dash: {slug}");
        assert!(slug.chars().count() <= 48);
    }

    #[test]
    fn short_hash_is_stable() {
        assert_eq!(short_hash("abc"), short_hash("abc"));
        assert_ne!(short_hash("abc"), short_hash("abd"));
    }
}
