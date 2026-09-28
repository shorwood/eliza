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
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 10
jsonpath "$.id" startsWith "resp_"
jsonpath "$.object" == "response"
jsonpath "$.status" == "completed"
jsonpath "$.error" == null
jsonpath "$.incomplete_details" == null
jsonpath "$.model" == "eliza-doctor"
jsonpath "$.output" count == 1
jsonpath "$.output[0].type" == "message"
jsonpath "$.output[0].id" startsWith "msg_"
jsonpath "$.output[0].status" == "completed"
jsonpath "$.output[0].role" == "assistant"
jsonpath "$.output[0].content" count == 1
jsonpath "$.output[0].content[0].type" == "output_text"
jsonpath "$.output[0].content[0].text" == "TOOL CALL COMPLETE"
jsonpath "$.output_text" == "TOOL CALL COMPLETE"
jsonpath "$.usage.input_tokens" isInteger
jsonpath "$.usage.output_tokens" isInteger
jsonpath "$.usage.total_tokens" isInteger
