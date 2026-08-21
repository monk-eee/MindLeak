//! Shared length-delimited byte-encoding for domain-separated signed
//! payloads (`claim_auth`, `knowledge_auth`).
//!
//! Extracted rather than left duplicated a second time: `claim_auth.rs`
//! wrote this once already, and a second signed-request domain
//! (`knowledge_auth.rs`, ADR-0108) needed the identical encoding rules --
//! the moment a helper is written twice in one crate is the signal to share
//! it, not fork it again for a third domain later.

/// A 4-byte big-endian length prefix followed by the field's own bytes, so a
/// signed payload can never be ambiguous about where one field ends and the
/// next begins.
pub(crate) fn push_field(bytes: &mut Vec<u8>, field: &[u8]) {
    bytes.extend_from_slice(&(field.len() as u32).to_be_bytes());
    bytes.extend_from_slice(field);
}

/// A count prefix followed by each element length-delimited, so
/// `paths=["a","b"], symbols=["c"]` can never encode the same bytes as
/// `paths=["a"], symbols=["b","c"]` -- a bare concatenation without the count
/// would let an attacker shift an element across the list boundary.
pub(crate) fn push_list(bytes: &mut Vec<u8>, items: &[String]) {
    bytes.extend_from_slice(&(items.len() as u32).to_be_bytes());
    for item in items {
        push_field(bytes, item.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_field_prefixes_the_exact_byte_length() {
        let mut bytes = Vec::new();
        push_field(&mut bytes, b"hi");
        assert_eq!(bytes, [0, 0, 0, 2, b'h', b'i']);
    }

    #[test]
    fn push_list_cannot_shift_an_element_across_the_list_boundary() {
        let mut a = Vec::new();
        push_list(&mut a, &["a".to_string(), "b".to_string()]);
        push_list(&mut a, &["c".to_string()]);

        let mut b = Vec::new();
        push_list(&mut b, &["a".to_string()]);
        push_list(&mut b, &["b".to_string(), "c".to_string()]);

        assert_ne!(a, b, "count prefixes must keep the two shapes distinct");
    }
}
