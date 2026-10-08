POST {{base_url}}/gemini/v1beta/models/eliza-retro-image:generateContent
x-goog-api-key: {{token}}
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "authenticated pixels"}]}],
  "generationConfig": {"responseModalities": ["IMAGE"]}
}
HTTP 200
[Asserts]
jsonpath "$.candidates[0].content.parts[0].inlineData.mimeType" == "image/png"
jsonpath "$.candidates[0].content.parts[0].inlineData.data" startsWith "iVBOR"
