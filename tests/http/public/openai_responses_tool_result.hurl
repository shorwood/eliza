POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-doctor",
  "tools": [{
    "type": "function",
    "name": "echo",
    "parameters": {"type": "object"}
  }],
  "input": [
    {
      "type": "message",
      "role": "user",
      "content": [{"type": "input_text", "text": "@tool echo {\"value\":\"hello\"}"}]
    },
    {
      "type": "function_call",
      "id": "fc_fixture",
      "call_id": "call_fixture",
      "name": "echo",
      "arguments": "{\"value\":\"hello\"}"
    },
    {"type": "function_call_output", "call_id": "call_fixture", "output": "hello"}
  ]
}
HTTP 200
[Asserts]
jsonpath "$.output_text" == "TOOL CALL COMPLETE"
