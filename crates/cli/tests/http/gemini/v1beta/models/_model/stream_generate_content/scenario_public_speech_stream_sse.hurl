POST {{base_url}}/gemini/v1beta/models/flite:streamGenerateContent?alt=sse
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "Streaming robot"}]}],
  "generationConfig": {
    "responseModalities": ["AUDIO"],
    "speechConfig": {"voiceConfig": {"voice": "retro"}}
  }
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "\"mimeType\":\"audio/L16;rate=24000\""
body contains "\"finishReason\":\"STOP\""
body contains "\"usageMetadata\""
