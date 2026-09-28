GET {{base_url}}/openai/v1/models
HTTP 200
[Asserts]
jsonpath "$.data[0].id" == "eliza-doctor"
