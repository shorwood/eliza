# Batch embeddings use the same literal model resource name.
POST {{base_url}}/gemini/v1beta/models/acme/embed:batchEmbedContents
Content-Type: application/json
{"requests":[{"model":"models/acme/embed","content":{"parts":[{"text":"Hello"}]},"outputDimensionality":8}]}
HTTP 200
[Asserts]
jsonpath "$.embeddings" count == 1
jsonpath "$.embeddings[0].values" count == 8
