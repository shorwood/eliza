POST {{base_url}}/openai/v1/images/generations
Content-Type: application/json
{"prompt": "pixels", "response_format": "url"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::unsupported_image_output_format"
jsonpath "$.error.param" == "response_format"
