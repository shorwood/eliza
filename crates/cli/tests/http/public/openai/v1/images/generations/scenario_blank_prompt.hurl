POST {{base_url}}/openai/v1/images/generations
Content-Type: application/json
{"prompt": "  \n  "}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::image::generation::blank_prompt"
jsonpath "$.error.param" == "prompt"
