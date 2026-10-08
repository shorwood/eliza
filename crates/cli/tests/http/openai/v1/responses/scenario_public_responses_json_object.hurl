POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": true,
  "input": "Hello",
  "text": {"format": {"type": "json_object"}}
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "\\\"response\\\""
body contains "\"type\":\"response.completed\""
