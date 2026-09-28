GET {{base_url}}/gemini/v1beta/models
HTTP 200
[Asserts]
jsonpath "$.models[0].name" == "models/eliza-doctor"
