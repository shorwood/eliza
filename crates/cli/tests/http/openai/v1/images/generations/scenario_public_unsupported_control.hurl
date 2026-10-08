POST {{base_url}}/openai/v1/images/generations
Content-Type: application/json
{"prompt": "pixels", "quality": "auto"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::unsupported_image_control"
jsonpath "$.error.param" == "quality"
