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
