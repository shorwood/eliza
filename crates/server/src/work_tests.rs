//! Native embedding preflight ignores unrelated provider fields.

use super::*;

// -----------------------------------------------------------------------------
// WorkTest: Prevents ignored wire fields weakening output admission.
// -----------------------------------------------------------------------------

/// Largest vector dimension exercised by resource preflight scenarios.
const WORK_TEST_VECTOR_DIMENSIONS: usize = 1024;

/// Configured aliases retain heavy-work admission, including encoded colons.
///
/// # Panics
/// Panics if merging alias support weakens hosted resource classification.
#[test]
fn work_test_configured_aliases_keep_media_admission() {
    let models = ModelAliases {
        images: eliza_http::model::ModelNames::new(vec!["image:fixture".parse().unwrap()]),
        speech: eliza_http::model::ModelNames::new(vec!["voice-fixture".parse().unwrap()]),
        ..ModelAliases::default()
    };
    let body = Bytes::from_static(b"{}");
    let image = WorkKinds::new(
        "/gemini/v1beta/models/image%3Afixture:generateContent",
        &Method::POST,
        &body,
        Some(&models),
    )
    .unwrap();
    assert!(image.has_image_work);
    let speech = WorkKinds::new(
        "/gemini/v1beta/models/voice-fixture:generateContent",
        &Method::POST,
        &body,
        Some(&models),
    )
    .unwrap();
    assert_eq!(speech.primary, Modality::Speech);
}

/// Native Gemini batches always count one vector per request and use native dimensions.
///
/// # Panics
/// Panics if ignored fields or nullable legacy controls weaken native output admission.
#[test]
fn work_test_gemini_ignores_openai_embedding_fields() {
    let mut value = serde_json::json!({"requests": [{
        "dimensions": 1, "input": [], "outputDimensionality": WORK_TEST_VECTOR_DIMENSIONS,
        "content": {"parts": [{"text": "Hello"}]}
    }]});
    let estimate =
        WorkKinds::embedding_output("/gemini/v1beta/models/fnv-embed:batchEmbedContents", &value);
    let expected = WORK_TEST_VECTOR_DIMENSIONS * WORK_POLICY_FLOAT_BYTES;
    assert_eq!(estimate, Some(expected));
    value["requests"][0]["outputDimensionality"] = Value::Null;
    value["requests"][0]["embedContentConfig"] =
        serde_json::json!({"outputDimensionality": WORK_TEST_VECTOR_DIMENSIONS});
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
    let value = serde_json::json!({"input": ["Hello", "World"], "dimensions": WORK_TEST_VECTOR_DIMENSIONS,
        "requests": [], "outputDimensionality": 1});
    let estimate = WorkKinds::embedding_output("/openai/v1/embeddings", &value);
    let expected = 2 * WORK_TEST_VECTOR_DIMENSIONS * WORK_POLICY_FLOAT_BYTES;
    assert_eq!(estimate, Some(expected));
}

/// Tool schemas and unrelated provider fields cannot masquerade as image input.
///
/// # Panics
/// Panics if metadata charges image admission or hides actual image content.
#[test]
fn work_test_image_admission_follows_native_content() {
    let value = serde_json::json!({"tools": [{"parameters": {"properties": {"inlineData": {"type": "string"}}}}],
        "input": [], "messages": [{"role": "user", "content": [{"type": "image_url", "image_url": {"url": "https://example.test/image.png"}}]}]});
    assert!(WorkKinds::has_request_images(
        "/openai/v1/chat/completions",
        &value
    ));
    assert!(!WorkKinds::has_request_images(
        "/openai/v1/responses",
        &value
    ));
    let metadata = serde_json::json!({"tools": [{"parameters": {"properties": {"inlineData": {"type": "string"}}}}],
        "messages": [{"role": "user", "content": "Hello"}]});
    assert!(!WorkKinds::has_request_images(
        "/openai/v1/chat/completions",
        &metadata
    ));
}

/// Nullable native Gemini inputs do not imply image decoding.
///
/// # Panics
/// Panics if absent native Gemini image inputs consume image admission.
#[test]
fn work_test_nullable_gemini_images_are_not_image_work() {
    let value = serde_json::json!({"contents": [{"parts": [{
        "text": "Hello", "inlineData": null, "fileData": null
    }]}]});
    let image =
        WorkKinds::has_request_images("/gemini/v1beta/models/eliza-1966:generateContent", &value);
    assert!(!image);
}

/// Positional nested configs cannot hide native audio or image selectors.
///
/// # Panics
/// Panics if alternate Serde structs bypass resource preflight.
#[test]
fn work_test_positional_generation_config_is_rejected() {
    let value = serde_json::json!({"contents": [{"parts": [{"text": "Hello"}]}],
        "generationConfig": [["AUDIO"], null, null, null, null]});
    let body = Bytes::from(serde_json::to_vec(&value).unwrap());
    let rejected = WorkKinds::new(
        "/gemini/v1beta/models/eliza-1966:generateContent",
        &Method::POST,
        &body,
        None,
    )
    .err()
    .unwrap();
    assert_eq!(rejected.status(), axum::http::StatusCode::BAD_REQUEST);
}

/// Ollama chat cannot charge images from an ignored top-level generate field.
///
/// # Panics
/// Panics if unrelated image locations consume admission or hide real generate images.
#[test]
fn work_test_ollama_image_locations_are_distinct() {
    let value = serde_json::json!({"images": ["fixture"], "messages": [{"content": "Hello"}]});
    assert!(!WorkKinds::has_request_images("/ollama/api/chat", &value));
    assert!(WorkKinds::has_request_images(
        "/ollama/api/generate",
        &value
    ));
}

/// Axum-decoded model/action parameters cannot bypass media or vector preflight.
///
/// # Panics
/// Panics if percent-encoded native routes evade resource classification.
#[test]
fn work_test_percent_encoded_model_actions_keep_admission() {
    let value = serde_json::json!({"contents": [{"parts": [{"text": "Hello"}]}]});
    let body = Bytes::from(serde_json::to_vec(&value).unwrap());
    let image = WorkKinds::new(
        "/gemini/v1beta/models/%65liza-retro-image%3AgenerateContent",
        &Method::POST,
        &body,
        None,
    )
    .unwrap();
    assert!(image.has_image_work);
    let value = serde_json::json!({"requests": [{"outputDimensionality": WORK_TEST_VECTOR_DIMENSIONS,
        "content": {"parts": [{"text": "Hello"}]}}]});
    let body = Bytes::from(serde_json::to_vec(&value).unwrap());
    let batch = WorkKinds::new(
        "/gemini/v1beta/models/fnv-embed%3AbatchEmbedContents",
        &Method::POST,
        &body,
        None,
    )
    .unwrap();
    assert_eq!(
        batch.estimated_output_bytes,
        Some(WORK_TEST_VECTOR_DIMENSIONS * WORK_POLICY_FLOAT_BYTES)
    );
}

/// Serde's alternate representations cannot escape heavy media or output admission.
///
/// # Panics
/// Panics if numeric image tags, positional messages or request tuples bypass admission.
#[test]
fn work_test_alternate_serde_shapes_remain_bounded() {
    let numeric = serde_json::json!({"messages": [{"content": [{"type": 1, "image_url": {"url": "fixture"}}]}]});
    assert!(WorkKinds::has_request_images(
        "/openai/v1/chat/completions",
        &numeric
    ));
    let positional = serde_json::json!({"messages": [["user", []]]});
    assert!(WorkKinds::has_request_images(
        "/openai/v1/chat/completions",
        &positional
    ));
    let root = Bytes::from_static(br#"["fnv-embed", ["Hello"], 8, null, null]"#);
    let rejected = WorkKinds::new("/openai/v1/embeddings", &Method::POST, &root, None).is_err();
    assert!(rejected);
    let batch = serde_json::json!({"requests": [[null, null, null, null, WORK_TEST_VECTOR_DIMENSIONS, null]]});
    let output =
        WorkKinds::embedding_output("/gemini/v1beta/models/fnv-embed:batchEmbedContents", &batch);
    assert_eq!(
        output,
        Some(WORK_TEST_VECTOR_DIMENSIONS * WORK_POLICY_FLOAT_BYTES)
    );
}
