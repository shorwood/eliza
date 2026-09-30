GET {{base_url}}/ollama/api/tags
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$" isObject
jsonpath "$.models" count == 2
jsonpath "$.models[0].*" count == 6
jsonpath "$.models[0].name" == "eliza-1966"
jsonpath "$.models[0].model" == "eliza-1966"
jsonpath "$.models[0].modified_at" == "1966-01-01T00:00:00Z"
jsonpath "$.models[0].size" == 0
jsonpath "$.models[0].digest" == "eliza-1966"
jsonpath "$.models[0].details.*" count == 6
jsonpath "$.models[0].details.parent_model" == ""
jsonpath "$.models[0].details.format" == "eliza"
jsonpath "$.models[0].details.family" == "eliza"
jsonpath "$.models[0].details.families" count == 1
jsonpath "$.models[0].details.families[0]" == "eliza"
jsonpath "$.models[0].details.parameter_size" == "DOCTOR"
jsonpath "$.models[0].details.quantization_level" == "none"
jsonpath "$.models[1].name" == "fnv-embed"
jsonpath "$.models[1].model" == "fnv-embed"
jsonpath "$.models[1].modified_at" == "1966-01-01T00:00:00Z"
jsonpath "$.models[1].size" == 0
jsonpath "$.models[1].digest" == "fnv-embed"
jsonpath "$.models[1].details.format" == "eliza"
jsonpath "$.models[1].details.family" == "eliza-embed"
jsonpath "$.models[1].details.families[0]" == "eliza-embed"
jsonpath "$.models[1].details.parameter_size" == "1024D"
jsonpath "$.models[1].details.quantization_level" == "none"
