POST {{base_url}}/openai/v1/images/generations
Content-Type: application/json
{"prompt": "pixels", "n": -1}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::image::generation::invalid_count"
jsonpath "$.error.param" == "n"
