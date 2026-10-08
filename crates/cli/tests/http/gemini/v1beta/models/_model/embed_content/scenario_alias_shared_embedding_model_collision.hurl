POST {{base_url}}/gemini/v1beta/models/fnv-embed:embedContent
Content-Type: application/json
{"model":"models/fnv-embed","content":{"parts":[{"text":"Hello world"}]},"outputDimensionality":8}
HTTP 200
[Asserts]
jsonpath "$.embedding.values" count == 8
jsonpath "$.embedding.values[1]" == -0.48507127
