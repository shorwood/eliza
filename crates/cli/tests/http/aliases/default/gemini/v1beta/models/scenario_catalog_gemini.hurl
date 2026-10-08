GET {{base_url}}/gemini/v1beta/models
x-goog-api-key: {{token}}
HTTP 200
[Asserts]
jsonpath "$.models" count == 9
jsonpath "$.models[0].name" == "models/chat-one"
jsonpath "$.models[1].name" == "models/chat-two"
jsonpath "$.models[2].name" == "models/voice-one"
jsonpath "$.models[3].name" == "models/voice-two"
jsonpath "$.models[4].name" == "models/embed-one"
jsonpath "$.models[4].supportedGenerationMethods[0]" == "embedContent"
jsonpath "$.models[5].name" == "models/embed-two"
jsonpath "$.models[6].name" == "models/embed{three}"
jsonpath "$.models[7].name" == "models/image-one"
jsonpath "$.models[8].name" == "models/image-two"
