GET {{base_url}}/ollama/api/tags
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.models" count == 5
jsonpath "$.models[0].name" == "chat-one"
jsonpath "$.models[1].name" == "chat-two"
jsonpath "$.models[2].name" == "embed-one"
jsonpath "$.models[2].details.family" == "eliza-embed"
jsonpath "$.models[3].name" == "embed-two"
jsonpath "$.models[4].name" == "embed{three}"
