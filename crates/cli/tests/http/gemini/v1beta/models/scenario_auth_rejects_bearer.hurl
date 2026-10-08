GET {{base_url}}/gemini/v1beta/models
Authorization: Bearer {{token}}
HTTP 401
[Asserts]
header "Content-Type" startsWith "application/json"
header "x-eliza-error-code" == "eliza::auth::failed"
jsonpath "$" isObject
jsonpath "$.error.*" count == 3
jsonpath "$.error.code" == 401
jsonpath "$.error.message" == "authentication failed"
jsonpath "$.error.status" == "UNAUTHENTICATED"
