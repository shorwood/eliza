GET {{base_url}}/openai/v1/models
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 9
jsonpath "$.data[0].id" == "chat-one"
jsonpath "$.data[1].id" == "chat-two"
jsonpath "$.data[2].id" == "voice-one"
jsonpath "$.data[3].id" == "voice-two"
jsonpath "$.data[4].id" == "embed-one"
jsonpath "$.data[5].id" == "embed-two"
jsonpath "$.data[6].id" == "embed{three}"
jsonpath "$.data[7].id" == "image-one"
jsonpath "$.data[8].id" == "image-two"
