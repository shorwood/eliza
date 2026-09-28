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
[Asserts]
jsonpath "$.output[0].type" == "function_call"
jsonpath "$.output[0].arguments" == "{\"value\":\"hello\"}"
