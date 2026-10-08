POST {{base_url}}/openai/v1/images/generations
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"eliza-retro-image","prompt":"retro cats","size":"256x256"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::image_generation_model_required"
jsonpath "$.error.message" contains "image-one"
