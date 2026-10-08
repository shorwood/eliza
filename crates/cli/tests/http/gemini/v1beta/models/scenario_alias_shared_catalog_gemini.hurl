GET {{base_url}}/gemini/v1beta/models
HTTP 200
[Asserts]
jsonpath "$.models" count == 3
jsonpath "$.models[0].name" == "models/fnv-embed"
jsonpath "$.models[0].displayName" == "ELIZA DOCTOR"
