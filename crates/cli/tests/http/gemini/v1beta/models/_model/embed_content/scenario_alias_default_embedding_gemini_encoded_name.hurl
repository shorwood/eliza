POST {{base_url}}/gemini/v1beta/models/embed%7Bthree%7D:embedContent
x-goog-api-key: {{token}}
Content-Type: application/json
{"model":"models/embed{three}","content":{"parts":[{"text":"Hello world"}]},"outputDimensionality":8}
HTTP 200
[Asserts]
jsonpath "$.embedding.values" count == 8
jsonpath "$.embedding.values[1]" == -0.48507127
