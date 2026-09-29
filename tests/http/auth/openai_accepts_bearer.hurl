GET {{base_url}}/openai/v1/models
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 2
jsonpath "$.object" == "list"
jsonpath "$.data" count == 2
jsonpath "$.data[0].id" == "eliza-doctor"
jsonpath "$.data[0].object" == "model"
