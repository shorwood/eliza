POST {{base_url}}/openai/v1/images/generations
Content-Type: application/json
{"prompt": "ordered variations", "n": 2, "size": "256x256"}
HTTP 200
[Captures]
variation_zero: jsonpath "$.data[0].b64_json"
variation_one: jsonpath "$.data[1].b64_json"
[Asserts]
jsonpath "$.data" count == 2
variable "variation_zero" != "{{variation_one}}"
