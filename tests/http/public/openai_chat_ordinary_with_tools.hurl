POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-doctor",
  "tools": [{
    "type": "function",
    "function": {"name": "echo", "parameters": {"type": "object"}}
  }],
  "messages": [{"role": "user", "content": "What is the weather?"}]
}
HTTP 200
[Asserts]
jsonpath "$.choices[0].message.tool_calls" not exists
jsonpath "$.choices[0].message.content" isString
jsonpath "$.choices[0].finish_reason" == "stop"
