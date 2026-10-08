POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": true,
  "messages": [{"role": "user", "content": "Hello"}],
  "response_format": {
    "type": "json_schema",
    "json_schema": {
      "name": "status",
      "schema": {
        "type": "object",
        "properties": {"ok": {"type": "boolean"}},
        "required": ["ok"]
      }
    }
  }
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "\\\"ok\\\":false"
body contains "data: [DONE]"
