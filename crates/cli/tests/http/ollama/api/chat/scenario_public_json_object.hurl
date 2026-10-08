POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-1966",
  "messages": [{"role": "user", "content": "Hello"}],
  "format": "json"
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/x-ndjson"
body contains "\\\"response\\\""
body contains "\"done\":true"
