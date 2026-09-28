GET {{base_url}}/openai/v1/models
x-api-key: {{token}}
HTTP 401
[Asserts]
header "x-eliza-error-code" == "eliza::auth::failed"
jsonpath "$.error.type" == "authentication_error"
jsonpath "$.error.code" == "eliza::auth::failed"
