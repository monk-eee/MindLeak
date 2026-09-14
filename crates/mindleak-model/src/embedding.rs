//! Storage-free decoding of OpenAI-compatible embedding responses.

use serde_json::Value;

use crate::{failure, ModelFailure, ModelFailureReason};

pub fn parse_embedding_response(
    value: &Value,
    expected_count: usize,
) -> Result<Vec<Vec<f32>>, ModelFailure> {
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("embeddings response missing data[]"))?;
    if data.len() != expected_count {
        return Err(invalid(format!(
            "embeddings returned {} vectors for {expected_count} inputs",
            data.len()
        )));
    }
    let mut vectors: Vec<Vec<f32>> = vec![Vec::new(); expected_count];
    let mut dimension = None;
    for (position, item) in data.iter().enumerate() {
        let index = match item.get("index") {
            None => position,
            Some(value) => value
                .as_u64()
                .and_then(|index| usize::try_from(index).ok())
                .ok_or_else(|| {
                    invalid("embeddings response index must be a nonnegative integer")
                })?,
        };
        if index >= vectors.len() {
            return Err(invalid("embeddings response index out of range"));
        }
        let components = item
            .get("embedding")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("embeddings response item missing embedding"))?;
        if components.is_empty() {
            return Err(invalid("empty embedding vector"));
        }
        let vector: Vec<f32> = components
            .iter()
            .enumerate()
            .map(|(position, value)| {
                let number = value.as_f64().ok_or_else(|| {
                    invalid(format!("embedding component {position} is not numeric"))
                })?;
                let narrowed = number as f32;
                if !number.is_finite() || !narrowed.is_finite() {
                    return Err(invalid(format!(
                        "embedding component {position} is not finite as f32"
                    )));
                }
                Ok(narrowed)
            })
            .collect::<Result<_, _>>()?;
        match dimension {
            Some(expected) if vector.len() != expected => {
                return Err(invalid(format!(
                    "embeddings response has inconsistent dimensions: expected {expected}, got {}",
                    vector.len()
                )));
            }
            None => dimension = Some(vector.len()),
            Some(_) => {}
        }
        vectors[index] = vector;
    }
    if vectors.iter().any(Vec::is_empty) {
        return Err(invalid("embeddings response was missing a vector"));
    }
    Ok(vectors)
}

fn invalid(detail: impl ToString) -> ModelFailure {
    failure(ModelFailureReason::BadJson, true, detail)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn indexed_results_are_restored_to_input_order() {
        let value = json!({"data": [
            {"index": 1, "embedding": [0.0, 1.0]},
            {"index": 0, "embedding": [1.0, 0.0]}
        ]});
        assert_eq!(
            parse_embedding_response(&value, 2).unwrap(),
            vec![vec![1.0, 0.0], vec![0.0, 1.0]]
        );
    }

    #[test]
    fn unindexed_results_preserve_response_order() {
        let value = json!({"data": [
            {"embedding": [1.0, 0.0]},
            {"embedding": [0.0, 1.0]}
        ]});
        assert_eq!(
            parse_embedding_response(&value, 2).unwrap(),
            vec![vec![1.0, 0.0], vec![0.0, 1.0]]
        );
        assert!(parse_embedding_response(&json!({"data": []}), 0)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn malformed_batches_are_refused_without_partial_vectors() {
        for value in [
            json!({}),
            json!({"data": []}),
            json!({"data": [{"index": 1, "embedding": [1.0]}]}),
            json!({"data": [{"index": 0}]}),
            json!({"data": [{"index": 0, "embedding": []}]}),
            json!({"data": [{"index": 0, "embedding": [1.0, "invalid"]}]}),
            json!({"data": [{"index": 0, "embedding": [3.5e38]}]}),
        ] {
            let error = parse_embedding_response(&value, 1).unwrap_err();
            assert_eq!(error.reason, ModelFailureReason::BadJson);
            assert!(error.reachable);
        }
    }

    #[test]
    fn duplicate_indices_and_inconsistent_dimensions_are_refused() {
        for value in [
            json!({"data": [
                {"index": 0, "embedding": [1.0]},
                {"index": 0, "embedding": [2.0]}
            ]}),
            json!({"data": [
                {"index": 0, "embedding": [1.0]},
                {"index": 1, "embedding": [0.0, 1.0]}
            ]}),
        ] {
            assert!(parse_embedding_response(&value, 2).is_err());
        }
    }

    // Regression: malformed explicit indices fell back to array order and could
    // attach a vector to the wrong label. Only an absent index may use position.
    #[test]
    fn a_malformed_explicit_index_is_not_treated_as_an_absent_index() {
        for index in [json!(-1), json!("0"), json!(0.5), Value::Null, json!(false)] {
            let response = json!({"data": [{"index": index, "embedding": [1.0]}]});
            assert!(
                parse_embedding_response(&response, 1).is_err(),
                "accepted index {index}"
            );
        }
    }
}
