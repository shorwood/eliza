GET {{base_url}}/gemini/v1beta/models
x-goog-api-key: {{token}}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$" isObject
jsonpath "$.models" count == 1
jsonpath "$.models[0].name" == "models/eliza-doctor"
jsonpath "$.models[0].version" == "1966-doctor"
jsonpath "$.models[0].supportedGenerationMethods" count == 2
