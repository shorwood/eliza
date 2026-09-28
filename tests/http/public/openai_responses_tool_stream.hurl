POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-doctor",
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
body contains "response.function_call_arguments.delta"
body contains "response.function_call_arguments.done"
