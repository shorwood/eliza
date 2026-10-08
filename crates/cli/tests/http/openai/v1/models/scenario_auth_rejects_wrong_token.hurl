# Reject a bearer credential that differs from the configured key.
GET {{base_url}}/openai/v1/models
Authorization: Bearer wrong
HTTP 401
[Asserts]
header "x-eliza-error-code" == "eliza::auth::failed"
jsonpath "$.error.type" == "authentication_error"
jsonpath "$.error.message" == "authentication failed"
