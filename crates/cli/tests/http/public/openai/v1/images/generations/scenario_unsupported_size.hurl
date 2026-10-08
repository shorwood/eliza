POST {{base_url}}/openai/v1/images/generations
Content-Type: application/json
{"prompt": "pixels", "size": "2048x2048"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::unsupported_image_size"
jsonpath "$.error.param" == "size"
