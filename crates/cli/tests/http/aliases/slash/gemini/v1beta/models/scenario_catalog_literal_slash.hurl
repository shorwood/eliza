# The catalog advertises literal model resource names.
GET {{base_url}}/gemini/v1beta/models
HTTP 200
[Asserts]
jsonpath "$.models[0].name" == "models/acme/chat"
jsonpath "$.models[2].name" == "models/acme/embed"
