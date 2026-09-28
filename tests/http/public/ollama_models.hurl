GET {{base_url}}/ollama/api/tags
HTTP 200
[Asserts]
jsonpath "$.models[0].model" == "eliza-doctor"
