POST {{base_url}}/openai/v1/images/generations
Content-Type: application/json
{"model": "eliza-1966", "prompt": "pixels"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::image_generation_model_required"
jsonpath "$.error.param" == "model"

POST {{base_url}}/openai/v1/images/generations
Content-Type: application/json
{"prompt": "pixels", "response_format": "url"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::unsupported_image_output_format"
jsonpath "$.error.param" == "response_format"

POST {{base_url}}/openai/v1/images/generations
Content-Type: application/json
{"prompt": "pixels", "output_format": "jpeg"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::unsupported_image_output_format"
jsonpath "$.error.param" == "output_format"

POST {{base_url}}/openai/v1/images/generations
Content-Type: application/json
{"prompt": "pixels", "size": "2048x2048"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::unsupported_image_size"
jsonpath "$.error.param" == "size"

POST {{base_url}}/openai/v1/images/generations
Content-Type: application/json
{"prompt": "pixels", "n": 5}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::image::generation::invalid_count"
jsonpath "$.error.param" == "n"

POST {{base_url}}/openai/v1/images/generations
Content-Type: application/json
{"prompt": "pixels", "quality": "auto"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::unsupported_image_control"
jsonpath "$.error.param" == "quality"

POST {{base_url}}/openai/v1/images/generations
Content-Type: application/json
{"prompt": "  \n  "}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::image::generation::blank_prompt"
jsonpath "$.error.param" == "prompt"

POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "pixels"}]}],
  "generationConfig": {"responseModalities": ["IMAGE"]}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::image_generation_model_required"
jsonpath "$.error.status" == "INVALID_ARGUMENT"

POST {{base_url}}/gemini/v1beta/models/eliza-retro-image:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "pixels"}]}],
  "generationConfig": {"responseModalities": ["TEXT"]}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::image_model_requires_image"

POST {{base_url}}/gemini/v1beta/models/eliza-retro-image:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "pixels"}]}],
  "generationConfig": {
    "responseModalities": ["IMAGE"],
    "responseFormat": {"image": {"aspectRatio": "16:9", "imageSize": "1K"}}
  }
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::unsupported_image_aspect_ratio"

POST {{base_url}}/gemini/v1beta/models/eliza-retro-image:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "pixels"}]}],
  "generationConfig": {
    "responseModalities": ["IMAGE"],
    "responseFormat": {"image": {"aspectRatio": "1:1", "imageSize": "2K"}}
  }
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::unsupported_image_size"

POST {{base_url}}/gemini/v1beta/models/eliza-retro-image:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"inlineData": {"mimeType": "image/png", "data": "iVBORw0KGgo="}}]}],
  "generationConfig": {"responseModalities": ["IMAGE"]}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::image_text_only"

POST {{base_url}}/gemini/v1beta/models/eliza-retro-image:generateContent
Content-Type: application/json
{
  "systemInstruction": {"parts": [{"text": "style it"}]},
  "contents": [{"role": "user", "parts": [{"text": "pixels"}]}],
  "generationConfig": {"responseModalities": ["IMAGE"]}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::unsupported_image_control"
