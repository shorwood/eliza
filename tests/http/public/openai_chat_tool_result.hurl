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
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 6
jsonpath "$.id" startsWith "chatcmpl-"
jsonpath "$.object" == "chat.completion"
jsonpath "$.model" == "eliza-doctor"
jsonpath "$.choices" count == 1
jsonpath "$.choices[0].index" == 0
jsonpath "$.choices[0].message.role" == "assistant"
jsonpath "$.choices[0].message.content" == "TOOL CALL COMPLETE"
jsonpath "$.choices[0].message.tool_calls" not exists
jsonpath "$.choices[0].finish_reason" == "stop"
jsonpath "$.usage.prompt_tokens" isInteger
jsonpath "$.usage.completion_tokens" isInteger
jsonpath "$.usage.total_tokens" isInteger
