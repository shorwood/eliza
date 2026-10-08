GET {{base_url}}/openai/v1/models
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 2
jsonpath "$.object" == "list"
jsonpath "$.data" count == 4
jsonpath "$.data[0].*" count == 4
jsonpath "$.data[0].id" == "eliza-1966"
jsonpath "$.data[0].object" == "model"
jsonpath "$.data[0].created" == 0
jsonpath "$.data[0].owned_by" == "eliza"
jsonpath "$.data[1].id" == "flite"
jsonpath "$.data[1].object" == "model"
jsonpath "$.data[1].created" == 0
jsonpath "$.data[1].owned_by" == "eliza"
jsonpath "$.data[2].id" == "fnv-embed"
jsonpath "$.data[2].object" == "model"
jsonpath "$.data[2].created" == 0
jsonpath "$.data[2].owned_by" == "eliza"
jsonpath "$.data[3].id" == "eliza-retro-image"
jsonpath "$.data[3].object" == "model"
jsonpath "$.data[3].created" == 0
jsonpath "$.data[3].owned_by" == "eliza"
