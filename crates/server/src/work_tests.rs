//! Native embedding preflight ignores unrelated provider fields.

use super::*;

// -----------------------------------------------------------------------------
// WorkTest: Prevents ignored wire fields weakening output admission.
// -----------------------------------------------------------------------------

/// Native Gemini batches always count one vector per request and use native dimensions.
///
/// # Panics
/// Panics if ignored fields or nullable legacy controls weaken native output admission.
#[test]
fn work_test_gemini_ignores_openai_embedding_fields() {
    let mut value = serde_json::json!({"requests": [{
        "dimensions": 1, "input": [], "outputDimensionality": 1024,
        "content": {"parts": [{"text": "Hello"}]}
    }]});
    let estimate =
        WorkKinds::embedding_output("/gemini/v1beta/models/fnv-embed:batchEmbedContents", &value);
    let expected = 1024 * WORK_POLICY_FLOAT_BYTES;
    assert_eq!(estimate, Some(expected));
    value["requests"][0]["outputDimensionality"] = Value::Null;
    value["requests"][0]["embedContentConfig"] = serde_json::json!({"outputDimensionality": 1024});
    let estimate =
        WorkKinds::embedding_output("/gemini/v1beta/models/fnv-embed:batchEmbedContents", &value);
    assert_eq!(estimate, Some(expected));
}

/// Unknown requests and native Gemini dimensions cannot replace `OpenAI` input and dimensions.
///
/// # Panics
/// Panics if ignored fields weaken the native vector estimate.
#[test]
fn work_test_openai_ignores_gemini_embedding_fields() {
    let value = serde_json::json!({"input": ["Hello", "World"], "dimensions": 1024,
        "requests": [], "outputDimensionality": 1});
    let estimate = WorkKinds::embedding_output("/openai/v1/embeddings", &value);
    let expected = 2 * 1024 * WORK_POLICY_FLOAT_BYTES;
    assert_eq!(estimate, Some(expected));
}
