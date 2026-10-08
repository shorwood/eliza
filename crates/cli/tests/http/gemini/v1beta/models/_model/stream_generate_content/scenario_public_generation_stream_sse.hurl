POST {{base_url}}/gemini/v1beta/models/eliza-retro-image:streamGenerateContent?alt=sse
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "sse pixels"}]}],
  "generationConfig": {"responseModalities": ["IMAGE", "TEXT"]}
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "data: {\"candidates\""
body contains "\"text\":"
body contains "\"inlineData\":{\"mimeType\":\"image/png\",\"data\":\"iVBOR"
body contains "\"finishReason\":\"STOP\""
body contains "\"modelVersion\":\"eliza-retro-image\""
