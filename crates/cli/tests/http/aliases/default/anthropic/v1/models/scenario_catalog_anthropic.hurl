GET {{base_url}}/anthropic/v1/models
x-api-key: {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 2
jsonpath "$.data[0].id" == "chat-one"
jsonpath "$.data[1].id" == "chat-two"
jsonpath "$.first_id" == "chat-one"
jsonpath "$.last_id" == "chat-two"
jsonpath "$.has_more" == false
