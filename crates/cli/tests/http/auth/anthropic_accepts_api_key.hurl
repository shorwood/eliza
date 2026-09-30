GET {{base_url}}/anthropic/v1/models
x-api-key: {{token}}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 4
jsonpath "$.data" count == 1
jsonpath "$.data[0].type" == "model"
jsonpath "$.data[0].id" == "eliza-1966"
jsonpath "$.has_more" == false
jsonpath "$.first_id" == "eliza-1966"
jsonpath "$.last_id" == "eliza-1966"
