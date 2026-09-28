POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-doctor",
  "stream": false,
  "tools": [{
    "type": "function",
    "function": {"name": "echo", "parameters": {"type": "object"}}
  }],
  "messages": [{"role": "user", "content": "@tool echo {\"value\":\"hello\"}"}]
}
HTTP 200
[Asserts]
jsonpath "$.message.tool_calls[0].function.name" == "echo"
jsonpath "$.message.tool_calls[0].function.arguments.value" == "hello"
