GET {{base_url}}/ollama/api/tags
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$" isObject
jsonpath "$.models" count == 1
jsonpath "$.models[0].*" count == 6
jsonpath "$.models[0].name" == "eliza-doctor"
jsonpath "$.models[0].model" == "eliza-doctor"
jsonpath "$.models[0].modified_at" == "1966-01-01T00:00:00Z"
jsonpath "$.models[0].size" == 0
jsonpath "$.models[0].digest" == "eliza-doctor"
jsonpath "$.models[0].details.*" count == 6
jsonpath "$.models[0].details.parent_model" == ""
jsonpath "$.models[0].details.format" == "eliza"
jsonpath "$.models[0].details.family" == "eliza"
jsonpath "$.models[0].details.families" count == 1
jsonpath "$.models[0].details.families[0]" == "eliza"
jsonpath "$.models[0].details.parameter_size" == "DOCTOR"
jsonpath "$.models[0].details.quantization_level" == "none"
