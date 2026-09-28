GET {{base_url}}/gemini/v1beta/models
Authorization: Bearer {{token}}
HTTP 401
[Asserts]
header "x-eliza-error-code" == "eliza::auth::failed"
jsonpath "$.error.code" == 401
jsonpath "$.error.status" == "UNAUTHENTICATED"
