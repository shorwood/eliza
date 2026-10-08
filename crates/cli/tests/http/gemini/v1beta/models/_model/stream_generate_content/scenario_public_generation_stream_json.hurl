POST {{base_url}}/gemini/v1beta/models/eliza-retro-image:streamGenerateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "json stream pixels"}]}],
  "generationConfig": {"responseModalities": ["IMAGE"]}
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$" count == 1
jsonpath "$[0].candidates[0].content.parts[0].inlineData.mimeType" == "image/png"
jsonpath "$[0].candidates[0].finishReason" == "STOP"
