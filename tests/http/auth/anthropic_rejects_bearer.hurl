GET {{base_url}}/anthropic/v1/models
Authorization: Bearer {{token}}
HTTP 401
[Asserts]
jsonpath "$.type" == "error"
