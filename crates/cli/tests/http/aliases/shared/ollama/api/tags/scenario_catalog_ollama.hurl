GET {{base_url}}/ollama/api/tags
HTTP 200
[Asserts]
jsonpath "$.models" count == 1
jsonpath "$.models[0].details.family" == "eliza"
