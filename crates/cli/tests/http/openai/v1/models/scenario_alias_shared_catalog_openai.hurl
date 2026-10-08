# Legacy chat renaming may overlap a dedicated model's default name.
GET {{base_url}}/openai/v1/models
HTTP 200
[Asserts]
jsonpath "$.data" count == 3
jsonpath "$.data[0].id" == "fnv-embed"
