POST {{base_url}}/gemini/v1beta/models/embed-one:embedContent
x-goog-api-key: {{token}}
Content-Type: application/json
{"model":"models/embed-two","content":{"parts":[{"text":"Hello world"}]}}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::embedding_model_required"
jsonpath "$.error.message" contains "models/embed-one"
