POST {{base_url}}/gemini/v1beta/models/fnv-embed:embedContent
Content-Type: application/json
{
  "model": "models/fnv-embed",
  "content": {"parts": [{"text": "Hello world"}]},
  "taskType": "SEMANTIC_SIMILARITY",
  "title": "ignored",
  "outputDimensionality": 8
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.embedding.values" count == 8
jsonpath "$.embedding.values[0]" == 0.0
jsonpath "$.embedding.values[1]" == -0.48507127
jsonpath "$.embedding.values[3]" == 0.24253564
jsonpath "$.embedding.values[7]" == 0.7276069
jsonpath "$.usageMetadata.promptTokenCount" == 2

POST {{base_url}}/gemini/v1beta/models/fnv-embed:batchEmbedContents
Content-Type: application/json
{
  "requests": [
    {
      "model": "models/fnv-embed",
      "content": {"parts": [{"text": "Hello world"}]},
      "embedContentConfig": {
        "taskType": "RETRIEVAL_QUERY",
        "title": "ignored",
        "outputDimensionality": 8,
        "autoTruncate": false
      }
    },
    {
      "model": "models/fnv-embed",
      "content": {"parts": [{"text": "Second"}, {"text": "text"}]},
      "outputDimensionality": 4
    }
  ]
}
HTTP 200
[Asserts]
jsonpath "$.embeddings" count == 2
jsonpath "$.embeddings[0].values" count == 8
jsonpath "$.embeddings[0].values[1]" == -0.48507127
jsonpath "$.embeddings[1].values" count == 4
jsonpath "$.usageMetadata.promptTokenCount" == 4

POST {{base_url}}/gemini/v1beta/models/fnv-embed:embedContent
Content-Type: application/json
{
  "model": "models/eliza-1966",
  "content": {"parts": [{"text": "Hello"}]}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::embedding_model_required"
jsonpath "$.error.status" == "INVALID_ARGUMENT"

POST {{base_url}}/gemini/v1beta/models/fnv-embed:embedContent
Content-Type: application/json
{
  "model": "models/fnv-embed",
  "content": {"parts": [{"inlineData": {"mimeType": "image/png", "data": "AA=="}}]}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::embedding_text_only"
jsonpath "$.error.status" == "INVALID_ARGUMENT"

POST {{base_url}}/gemini/v1beta/models/fnv-embed:embedContent
Content-Type: application/json
{
  "model": "models/fnv-embed",
  "content": {"parts": [{"text": "Hello"}]},
  "outputDimensionality": 8,
  "embedContentConfig": {"outputDimensionality": 4}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::conflicting_embedding_dimensions"
jsonpath "$.error.status" == "INVALID_ARGUMENT"

POST {{base_url}}/gemini/v1beta/models/fnv-embed:batchEmbedContents
Content-Type: application/json
{"requests": []}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::embedding::empty_batch"
jsonpath "$.error.status" == "INVALID_ARGUMENT"

POST {{base_url}}/gemini/v1beta/models/fnv-embed:embedContent
Content-Type: application/json
{
  "model": "models/fnv-embed",
  "content": {"parts": [{"text": "Hello"}]},
  "embedContentConfig": {"autoTruncate": true}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::unsupported_embedding_control"
jsonpath "$.error.status" == "INVALID_ARGUMENT"
jsonpath "$.error.message" == "unsupported embedding control `embedContentConfig.autoTruncate`"
