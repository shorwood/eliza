POST {{base_url}}/gemini/v1beta/openai/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": true,
  "messages": [{"role": "user", "content": "Hello"}],
  "response_format": {"type": "json_object"}
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "\\\"response\\\""
