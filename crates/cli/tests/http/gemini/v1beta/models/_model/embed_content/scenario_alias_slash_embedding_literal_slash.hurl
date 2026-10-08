# A literal slash in the embedding name must route.
POST {{base_url}}/gemini/v1beta/models/acme/embed:embedContent
Content-Type: application/json
{"model":"models/acme/embed","content":{"parts":[{"text":"Hello"}]},"outputDimensionality":8}
HTTP 200
[Asserts]
jsonpath "$.embedding.values" count == 8
