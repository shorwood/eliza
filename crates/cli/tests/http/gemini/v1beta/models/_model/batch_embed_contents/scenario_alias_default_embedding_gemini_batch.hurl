POST {{base_url}}/gemini/v1beta/models/embed-two:batchEmbedContents
x-goog-api-key: {{token}}
Content-Type: application/json
{"requests":[
  {"model":"models/embed-two","content":{"parts":[{"text":"Hello world"}]},"outputDimensionality":8},
  {"model":"models/embed-two","content":{"parts":[{"text":"Another text"}]},"outputDimensionality":8}
]}
HTTP 200
[Asserts]
jsonpath "$.embeddings" count == 2
jsonpath "$.embeddings[0].values[1]" == -0.48507127
