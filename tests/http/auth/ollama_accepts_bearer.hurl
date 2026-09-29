GET {{base_url}}/ollama/api/tags
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$" isObject
jsonpath "$.models" count == 1
jsonpath "$.models[0].name" == "eliza-1966"
jsonpath "$.models[0].model" == "eliza-1966"
jsonpath "$.models[0].details.family" == "eliza"
