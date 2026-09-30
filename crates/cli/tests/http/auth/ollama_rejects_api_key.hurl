GET {{base_url}}/ollama/api/tags
x-api-key: {{token}}
HTTP 401
[Asserts]
header "Content-Type" startsWith "application/json"
header "x-eliza-error-code" == "eliza::auth::failed"
jsonpath "$" isObject
jsonpath "$.error" == "authentication failed"
