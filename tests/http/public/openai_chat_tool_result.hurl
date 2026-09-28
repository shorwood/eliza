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
        "id": "call_fixture",
        "type": "function",
        "function": {"name": "echo", "arguments": "{\"value\":\"hello\"}"}
      }]
    },
    {"role": "tool", "tool_call_id": "call_fixture", "content": "hello"}
  ]
}
HTTP 200
[Asserts]
jsonpath "$.choices[0].message.content" == "TOOL CALL COMPLETE"
