GET {{base_url}}/anthropic/v1/models
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 4
jsonpath "$.data" count == 1
jsonpath "$.data[0].*" count == 4
jsonpath "$.data[0].type" == "model"
jsonpath "$.data[0].id" == "eliza-1966"
jsonpath "$.data[0].display_name" == "ELIZA DOCTOR"
jsonpath "$.data[0].created_at" == "1966-01-01T00:00:00Z"
jsonpath "$.has_more" == false
jsonpath "$.first_id" == "eliza-1966"
jsonpath "$.last_id" == "eliza-1966"
