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
