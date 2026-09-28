POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-doctor",
  "messages": [{"role": "user", "content": "Hello"}],
  "future_field": true
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/x-ndjson"
body contains "\"done\":false"
body contains "\"done\":true"
body contains "\"done_reason\":\"stop\""
