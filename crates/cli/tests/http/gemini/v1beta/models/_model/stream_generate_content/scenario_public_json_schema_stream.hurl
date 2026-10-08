POST {{base_url}}/gemini/v1beta/models/eliza-1966:streamGenerateContent?alt=sse
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "Hello"}]}],
  "generationConfig": {
    "responseFormat": {
      "text": {
        "mimeType": "APPLICATION_JSON",
        "schema": {
          "type": "object",
          "properties": {"ok": {"type": "boolean"}},
          "required": ["ok"]
        }
      }
    }
  }
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "\\\"ok\\\":false"
body contains "\"finishReason\":\"STOP\""
