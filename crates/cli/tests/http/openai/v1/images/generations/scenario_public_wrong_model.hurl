POST {{base_url}}/openai/v1/images/generations
Content-Type: application/json
{"model": "eliza-1966", "prompt": "pixels"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::image_generation_model_required"
jsonpath "$.error.param" == "model"
