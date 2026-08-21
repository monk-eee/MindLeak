use std::collections::HashSet;

use super::KnowledgeStoreError;

pub(super) fn validate_reach(
    reach_node_ids: &[String],
    reach_goal_id: Option<&str>,
) -> Result<(), KnowledgeStoreError> {
    let mut seen = HashSet::new();
    for node_id in reach_node_ids {
        if !valid_repository_node(node_id) {
            return Err(KnowledgeStoreError::InvalidReachNode(node_id.clone()));
        }
        if !seen.insert(node_id.as_str()) {
            return Err(KnowledgeStoreError::DuplicateReachNode(node_id.clone()));
        }
    }
    if let Some(goal_id) = reach_goal_id {
        if !valid_goal_id(goal_id) {
            return Err(KnowledgeStoreError::InvalidReachGoal);
        }
    }
    Ok(())
}

fn valid_repository_node(id: &str) -> bool {
    if let Some(path) = id.strip_prefix("artifact:") {
        return valid_repository_path(path);
    }
    if let Some(symbol) = id.strip_prefix("symbol:") {
        let Some((path, name)) = symbol.split_once(':') else {
            return false;
        };
        return !name.is_empty() && valid_repository_path(path);
    }
    false
}

fn valid_repository_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.contains(':')
        && !value
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
}

fn valid_goal_id(id: &str) -> bool {
    id.strip_prefix("goal:")
        .is_some_and(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
}

#[cfg(test)]
mod tests {
    use super::validate_reach;
    use crate::knowledge_store::KnowledgeStoreError;

    #[test]
    fn rejects_a_non_repository_relative_reach_node() {
        let invalid_node = "artifact:../outside-the-repository".to_string();

        let error = validate_reach(std::slice::from_ref(&invalid_node), None).unwrap_err();

        assert!(matches!(
            error,
            KnowledgeStoreError::InvalidReachNode(node) if node == invalid_node
        ));
    }

    #[test]
    fn rejects_duplicate_reach_nodes() {
        let node = "artifact:crates/ackplane-server/src/knowledge_store.rs".to_string();

        let error = validate_reach(&[node.clone(), node.clone()], None).unwrap_err();

        assert!(matches!(
            error,
            KnowledgeStoreError::DuplicateReachNode(duplicate) if duplicate == node
        ));
    }

    #[test]
    fn rejects_a_non_goal_reach_identifier() {
        let error = validate_reach(&[], Some("goal:contains a space")).unwrap_err();

        assert!(matches!(error, KnowledgeStoreError::InvalidReachGoal));
    }

    #[test]
    fn rejects_a_symbol_without_its_repository_path_and_name_separator() {
        let invalid_node = "symbol:crates/ackplane-server/src/knowledge_store.rs".to_string();

        let error = validate_reach(std::slice::from_ref(&invalid_node), None).unwrap_err();

        assert!(matches!(
            error,
            KnowledgeStoreError::InvalidReachNode(node) if node == invalid_node
        ));
    }

    #[test]
    fn rejects_a_colon_bearing_artifact_path() {
        let invalid_node = "artifact:crates/ackplane-server:knowledge_store.rs".to_string();

        let error = validate_reach(std::slice::from_ref(&invalid_node), None).unwrap_err();

        assert!(matches!(
            error,
            KnowledgeStoreError::InvalidReachNode(node) if node == invalid_node
        ));
    }
}
