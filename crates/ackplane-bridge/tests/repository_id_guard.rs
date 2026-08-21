//! Structural guard for ADR-0098 decision 5: every Bridge route handler and
//! every `FleetStore` query/mutation must carry an explicit tenant scope.
//! This is a lint/test-level guard, not a permissions engine - it does not
//! check WHO may call a route, only that every route this Bridge serves, and
//! every read-store method it can reach, is scoped to a tenant rather than
//! trusting whoever adds the next one to remember.

const MAIN_RS: &str = include_str!("../src/main.rs");
const FLEET_RS: &str = include_str!("../../ackplane-server/src/fleet.rs");
const KNOWLEDGE_STORE_RS: &str = include_str!("../../ackplane-server/src/knowledge_store.rs");
const CLAIM_STORE_RS: &str = include_str!("../../ackplane-server/src/claim_store.rs");

/// `FleetStore::connect` is the connection constructor, not a query or
/// mutation - it has no tenant to scope to yet.
const FLEET_STORE_METHODS_WITHOUT_A_TENANT: &[&str] = &["connect"];

/// `KnowledgeStore::connect` is the connection constructor - same exemption
/// as `FleetStore::connect` above. `record` is exempt for a different reason:
/// it takes a `RecordKnowledgeRequest` whose own `tenant_id: String` field
/// still carries the scope - the guard's plain-text check only recognises a
/// direct `tenant_id: &str` parameter, not one nested inside a request struct.
/// `resolve_signing_key` is exempt the same way `record` is: its tenant scope
/// lives in `EnvelopeBinding::tenant_id`, judged by `signing_keys::resolve`,
/// not a bare parameter on this method. `consume_knowledge_nonce` needs no
/// tenant_id at all - it is keyed by `(signing_key_id, nonce)`, and
/// `signing_key_id` is already globally unique (`signing_keys` enforces
/// `ON CONFLICT (signing_key_id)`), so no two tenants can ever share one to
/// collide across.
const KNOWLEDGE_STORE_METHODS_WITHOUT_A_TENANT: &[&str] = &[
    "connect",
    "record",
    "resolve_signing_key",
    "consume_knowledge_nonce",
];

/// `fleet_page` serves a static asset and never touches the store - it has
/// nothing to scope.
const ROUTE_HANDLERS_WITHOUT_A_STORE_QUERY: &[&str] = &["fleet_page"];

/// `connect` is the constructor, same exemption as the other two stores.
/// `delegate` and `recover` take a request struct (`ClaimLeaseRequest`,
/// `ClaimRecoverRequest`) whose own `tenant_id: String` field carries the
/// scope - the same struct-embedded exemption reasoning as `KnowledgeStore::
/// record` above. `resolve_signing_key` takes an `EnvelopeBinding` rather
/// than a bare tenant id, for the same reason. `consume_claim_nonce` has no
/// tenant scope at all by design: nonce uniqueness is global on
/// (signing_key_id, nonce), not tenant-scoped (matching
/// `activation_challenges.nonce`'s existing global-uniqueness precedent).
const CLAIM_STORE_METHODS_WITHOUT_A_TENANT: &[&str] = &[
    "connect",
    "delegate",
    "recover",
    "resolve_signing_key",
    "consume_claim_nonce",
];

#[test]
fn every_claim_store_query_requires_an_explicit_tenant_id() {
    let methods = extract_impl_methods(CLAIM_STORE_RS, "ClaimStore");
    assert!(
        !methods.is_empty(),
        "expected to find at least one ClaimStore method - the parser may be broken"
    );
    for (name, signature) in methods {
        if CLAIM_STORE_METHODS_WITHOUT_A_TENANT.contains(&name.as_str()) {
            continue;
        }
        assert!(
            signature.contains("tenant_id: &str"),
            "ClaimStore::{name} does not take an explicit tenant_id: &str parameter \
             (ADR-0098 decision 5 requires every query/mutation to carry a tenant scope): {signature}"
        );
    }
}

#[test]
fn every_fleet_store_query_requires_an_explicit_tenant_id() {
    let methods = extract_impl_methods(FLEET_RS, "FleetStore");
    assert!(
        !methods.is_empty(),
        "expected to find at least one FleetStore method - the parser may be broken"
    );
    for (name, signature) in methods {
        if FLEET_STORE_METHODS_WITHOUT_A_TENANT.contains(&name.as_str()) {
            continue;
        }
        assert!(
            signature.contains("tenant_id: &str"),
            "FleetStore::{name} does not take an explicit tenant_id: &str parameter \
             (ADR-0098 decision 5 requires every query/mutation to carry a tenant scope): {signature}"
        );
    }
}

#[test]
fn every_knowledge_store_query_requires_an_explicit_tenant_id() {
    let methods = extract_impl_methods(KNOWLEDGE_STORE_RS, "KnowledgeStore");
    assert!(
        !methods.is_empty(),
        "expected to find at least one KnowledgeStore method - the parser may be broken"
    );
    for (name, signature) in methods {
        if KNOWLEDGE_STORE_METHODS_WITHOUT_A_TENANT.contains(&name.as_str()) {
            continue;
        }
        assert!(
            signature.contains("tenant_id: &str"),
            "KnowledgeStore::{name} does not take an explicit tenant_id: &str parameter \
             (ADR-0098 decision 5 requires every query/mutation to carry a tenant scope): {signature}"
        );
    }
}

#[test]
fn every_bridge_route_handler_scopes_its_query_to_the_tenant() {
    let handlers = extract_route_handlers(MAIN_RS);
    assert!(
        !handlers.is_empty(),
        "expected to find at least one Bridge route - the parser may be broken"
    );
    for handler in handlers {
        if ROUTE_HANDLERS_WITHOUT_A_STORE_QUERY.contains(&handler.as_str()) {
            continue;
        }
        let body = extract_function_body(MAIN_RS, &handler).unwrap_or_else(|| {
            panic!("could not find the body of handler `{handler}` registered on a route")
        });
        assert!(
            body.contains("state.tenant_id"),
            "Bridge route handler `{handler}` never references state.tenant_id \
             (ADR-0098 decision 5 requires every query/mutation to carry a tenant scope)"
        );
    }
}

/// Every `pub async fn NAME(...)` inside `impl <type_name> { ... }`, returning
/// each method's name and its full (possibly multi-line) parameter list.
fn extract_impl_methods(source: &str, type_name: &str) -> Vec<(String, String)> {
    let impl_marker = format!("impl {type_name} {{");
    let impl_start = source
        .find(&impl_marker)
        .unwrap_or_else(|| panic!("expected an `impl {type_name} {{` block"));
    let impl_open = impl_start + impl_marker.len() - 1;
    let impl_close = balanced_braces(source, impl_open)
        .unwrap_or_else(|| panic!("`impl {type_name}` block is never closed"));
    let impl_body = &source[impl_open..impl_close];

    let marker = "pub async fn ";
    let mut methods = Vec::new();
    let mut cursor = 0;
    while let Some(start) = impl_body[cursor..].find(marker) {
        let after_marker = cursor + start + marker.len();
        let name_end = impl_body[after_marker..]
            .find('(')
            .expect("a fn name is followed by (");
        let name = impl_body[after_marker..after_marker + name_end].to_string();
        let params_start = after_marker + name_end;
        let params_end = balanced_parens(impl_body, params_start)
            .unwrap_or_else(|| panic!("FleetStore::{name}'s parameter list is never closed"));
        methods.push((name, impl_body[params_start..params_end].to_string()));
        cursor = params_end;
    }
    methods
}

/// Every handler name passed to `get(...)` on a registered route.
fn extract_route_handlers(source: &str) -> Vec<String> {
    let marker = "get(";
    let mut handlers = Vec::new();
    let mut cursor = 0;
    while let Some(start) = source[cursor..].find(marker) {
        let name_start = cursor + start + marker.len();
        let name_end = source[name_start..].find(')').expect("get(...) is closed");
        handlers.push(source[name_start..name_start + name_end].trim().to_string());
        cursor = name_start + name_end;
    }
    handlers
}

/// The full brace-balanced body of `async fn NAME(...) ... { ... }`.
fn extract_function_body(source: &str, name: &str) -> Option<String> {
    let marker = format!("async fn {name}(");
    let start = source.find(&marker)?;
    let brace_start = source[start..].find('{')? + start;
    let end = balanced_braces(source, brace_start)?;
    Some(source[brace_start..end].to_string())
}

/// The index just past the `)` that matches the `(` at `open`.
fn balanced_parens(source: &str, open: usize) -> Option<usize> {
    let mut depth = 0_i32;
    for (offset, ch) in source[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// The index just past the `}` that matches the `{` at `open`.
fn balanced_braces(source: &str, open: usize) -> Option<usize> {
    let mut depth = 0_i32;
    for (offset, ch) in source[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset + 1);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod parser_tests {
    use super::*;

    #[test]
    fn extract_impl_methods_finds_every_method_and_its_full_signature() {
        let sample = "\
            impl Store {\n\
            \x20   pub async fn connect(url: &str) -> Result<Self, Error> { todo!() }\n\
            \x20   pub async fn scoped(\n\
            \x20       &self,\n\
            \x20       tenant_id: &str,\n\
            \x20   ) -> Result<(), Error> { todo!() }\n\
            }\n\
            impl OtherType {\n\
            \x20   pub async fn unrelated(&self) {}\n\
            }";

        let methods = extract_impl_methods(sample, "Store");

        assert_eq!(methods.len(), 2);
        assert_eq!(methods[0].0, "connect");
        assert!(!methods[0].1.contains("tenant_id"));
        assert_eq!(methods[1].0, "scoped");
        assert!(methods[1].1.contains("tenant_id: &str"));
    }

    #[test]
    fn extract_route_handlers_reads_every_get_handler_in_order() {
        let sample = "Router::new()\n\
            .route(\"/\", get(page))\n\
            .route(\"/api/v1/thing\", get(thing_handler))";

        assert_eq!(
            extract_route_handlers(sample),
            vec!["page".to_string(), "thing_handler".to_string()]
        );
    }

    #[test]
    fn extract_function_body_is_balanced_across_nested_braces() {
        let sample = "async fn handler(x: i32) -> i32 { if x > 0 { x } else { 0 } }";

        let body = extract_function_body(sample, "handler").expect("handler body found");

        assert_eq!(body, "{ if x > 0 { x } else { 0 } }");
    }

    #[test]
    fn extract_function_body_returns_none_for_a_missing_handler() {
        assert_eq!(
            extract_function_body("async fn present() {}", "absent"),
            None
        );
    }
}
