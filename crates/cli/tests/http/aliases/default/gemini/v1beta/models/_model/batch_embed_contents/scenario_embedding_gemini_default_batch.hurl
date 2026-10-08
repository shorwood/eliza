POST {{base_url}}/gemini/v1beta/models/embed-one:batchEmbedContents
x-goog-api-key: {{token}}
Content-Type: application/json
{"requests":[
  {"model":"models/embed-one","content":{"parts":[{"text":"Hello world"}]}},
  {"model":"models/embed-two","content":{"parts":[{"text":"Another text"}]}}
]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::embedding_model_required"
