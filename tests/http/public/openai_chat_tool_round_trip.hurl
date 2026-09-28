POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-doctor",
  "tools": [{
    "type": "function",
    "function": {
      "name": "echo",
      "description": "Echo a value",
      "parameters": {
        "type": "object",
        "properties": {"value": {"type": "string"}},
        "required": ["value"]
      }
    }
  }],
  "messages": [{"role": "user", "content": "@tool echo {\"value\":\"hello\"}"}]
}
HTTP 200
[Captures]
chat_call_id: jsonpath "$.choices[0].message.tool_calls[0].id"
[Asserts]
jsonpath "$.choices[0].finish_reason" == "tool_calls"
jsonpath "$.choices[0].message.tool_calls[0].function.name" == "echo"
jsonpath "$.choices[0].message.tool_calls[0].function.arguments" == "{\"value\":\"hello\"}"

POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-doctor",
  "tools": [{
    "type": "function",
    "function": {"name": "echo", "parameters": {"type": "object"}}
  }],
  "messages": [
    {"role": "user", "content": "@tool echo {\"value\":\"hello\"}"},
    {
      "role": "assistant",
      "content": null,
      "tool_calls": [{
        "id": "{{chat_call_id}}",
        "type": "function",
        "function": {"name": "echo", "arguments": "{\"value\":\"hello\"}"}
      }]
    },
    {"role": "tool", "tool_call_id": "{{chat_call_id}}", "content": "hello"}
  ]
}
HTTP 200
[Asserts]
jsonpath "$.choices[0].message.content" == "TOOL CALL COMPLETE"
