POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": true,
  "tools": [{
    "type": "function",
    "name": "echo",
    "parameters": {"type": "object"}
  }],
  "input": "@tool echo {\"value\":\"hello\"}"
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "\"type\":\"response.created\""
body contains "\"model\":\"eliza-1966\""
body contains "\"type\":\"response.output_item.added\""
body contains "\"type\":\"response.function_call_arguments.delta\""
body contains "\"type\":\"response.function_call_arguments.done\""
body contains "\"name\":\"echo\""
body contains "\"arguments\":\"{\\\"value\\\":\\\"hello\\\"}\""
body contains "\"type\":\"response.output_item.done\""
body contains "\"type\":\"response.completed\""
body contains "\"status\":\"completed\""
