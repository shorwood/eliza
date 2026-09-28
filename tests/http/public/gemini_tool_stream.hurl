POST {{base_url}}/gemini/v1beta/models/eliza-doctor:streamGenerateContent?alt=sse
Content-Type: application/json
{
  "tools": [{"functionDeclarations": [{
    "name": "echo",
    "parameters": {"type": "object"}
  }]}],
  "contents": [{"role": "user", "parts": [{
    "text": "@tool echo {\"value\":\"hello\"}"
  }]}]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "data: {\"candidates\""
body contains "\"role\":\"model\""
body contains "\"functionCall\""
body contains "\"name\":\"echo\""
body contains "\"args\":{\"value\":\"hello\"}"
body contains "\"modelVersion\":\"eliza-doctor\""
body contains "\"finishReason\":\"STOP\""
body contains "\"usageMetadata\""
