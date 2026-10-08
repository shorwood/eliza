POST {{base_url}}/gemini/v1beta/models/flite:streamGenerateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "Streaming JSON robot"}]}],
  "generationConfig": {
    "responseModalities": ["AUDIO"],
    "speechConfig": {"voiceConfig": {"voice": "retro"}}
  }
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$[0].candidates[0].content.parts[0].inlineData.mimeType" == "audio/L16;rate=24000"
body contains "\"finishReason\":\"STOP\""
body contains "\"usageMetadata\""
