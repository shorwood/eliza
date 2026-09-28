GET {{base_url}}/anthropic/v1/models
Authorization: Bearer {{token}}
HTTP 401
[Asserts]
header "Content-Type" startsWith "application/json"
header "x-eliza-error-code" == "eliza::auth::failed"
jsonpath "$.*" count == 2
jsonpath "$.type" == "error"
jsonpath "$.error.*" count == 2
jsonpath "$.error.type" == "authentication_error"
jsonpath "$.error.message" == "authentication failed"
