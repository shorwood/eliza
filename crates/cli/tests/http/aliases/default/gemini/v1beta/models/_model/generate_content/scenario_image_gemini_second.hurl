POST {{base_url}}/gemini/v1beta/models/image-two:generateContent
x-goog-api-key: {{token}}
Content-Type: application/json
{"contents":[{"role":"user","parts":[{"text":"retro cats"}]}],"generationConfig":{"responseModalities":["TEXT"]}}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::image_model_requires_image"
jsonpath "$.error.message" contains "image-two"
