GET {{base_url}}/anthropic/v1/models
Authorization: Bearer {{token}}
HTTP 401
[Asserts]
header "x-eliza-error-code" == "eliza::auth::failed"
jsonpath "$.type" == "error"
jsonpath "$.error.type" == "authentication_error"
