POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-doctor",
  "tools": [{
    "type": "function",
    "name": "echo",
    "description": "Echo a value",
    "parameters": {"type": "object"}
  }],
  "input": "@tool echo {\"value\":\"hello\"}"
}
HTTP 200
[Captures]
response_call_id: jsonpath "$.output[0].call_id"
[Asserts]
jsonpath "$.output[0].type" == "function_call"
jsonpath "$.output[0].arguments" == "{\"value\":\"hello\"}"

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
      "call_id": "{{response_call_id}}",
      "name": "echo",
      "arguments": "{\"value\":\"hello\"}"
    },
    {"type": "function_call_output", "call_id": "{{response_call_id}}", "output": "hello"}
  ]
}
HTTP 200
[Asserts]
jsonpath "$.output_text" == "TOOL CALL COMPLETE"
