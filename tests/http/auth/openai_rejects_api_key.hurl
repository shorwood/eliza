GET {{base_url}}/openai/v1/models
x-api-key: {{token}}
HTTP 401
[Asserts]
jsonpath "$.error.type" == "authentication_error"
