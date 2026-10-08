POST {{base_url}}/gemini/v1beta/models/eliza-retro-image:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"inlineData": {"mimeType": "image/png", "data": "iVBORw0KGgo="}}]}],
  "generationConfig": {"responseModalities": ["IMAGE"]}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::image_text_only"
