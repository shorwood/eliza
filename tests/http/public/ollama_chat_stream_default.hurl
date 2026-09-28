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
body contains "\"model\":\"eliza-doctor\""
body contains "\"created_at\":\"1966-01-01T00:00:00Z\""
body contains "\"role\":\"assistant\""
body contains "\"done\":false"
body contains "\"done\":true"
body contains "\"done_reason\":\"stop\""
body contains "\"prompt_eval_count\""
body contains "\"eval_count\""
