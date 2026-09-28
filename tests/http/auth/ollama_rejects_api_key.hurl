GET {{base_url}}/ollama/api/tags
x-api-key: {{token}}
HTTP 401
[Asserts]
header "x-eliza-error-code" == "eliza::auth::failed"
jsonpath "$.error" == "authentication failed"
