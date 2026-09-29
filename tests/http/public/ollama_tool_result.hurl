POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": false,
  "tools": [{
    "type": "function",
    "function": {"name": "echo", "parameters": {"type": "object"}}
  }],
  "messages": [
    {"role": "user", "content": "@tool echo {\"value\":\"hello\"}"},
    {"role": "assistant", "content": "", "tool_calls": [{
      "type": "function",
      "function": {"name": "echo", "arguments": {"value": "hello"}}
    }]},
    {"role": "tool", "tool_name": "echo", "content": "hello"}
  ]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 11
jsonpath "$.model" == "eliza-1966"
jsonpath "$.created_at" == "1966-01-01T00:00:00Z"
jsonpath "$.message.role" == "assistant"
jsonpath "$.message.content" == "TOOL CALL COMPLETE"
jsonpath "$.message.tool_calls" not exists
jsonpath "$.done" == true
jsonpath "$.done_reason" == "stop"
jsonpath "$.total_duration" == 0
jsonpath "$.load_duration" == 0
jsonpath "$.prompt_eval_count" isInteger
jsonpath "$.prompt_eval_duration" == 0
jsonpath "$.eval_count" isInteger
jsonpath "$.eval_duration" == 0
