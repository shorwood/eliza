GET {{base_url}}/openai/v1/models
x-api-key: {{token}}
HTTP 401
[Asserts]
header "Content-Type" startsWith "application/json"
header "x-eliza-error-code" == "eliza::auth::failed"
jsonpath "$" isObject
jsonpath "$.error.*" count == 4
jsonpath "$.error.message" == "authentication failed"
jsonpath "$.error.type" == "authentication_error"
jsonpath "$.error.param" == null
jsonpath "$.error.code" == "eliza::auth::failed"
