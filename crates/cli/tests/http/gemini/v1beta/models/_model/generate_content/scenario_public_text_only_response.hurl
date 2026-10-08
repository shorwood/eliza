POST {{base_url}}/gemini/v1beta/models/eliza-retro-image:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "pixels"}]}],
  "generationConfig": {"responseModalities": ["TEXT"]}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::image_model_requires_image"
