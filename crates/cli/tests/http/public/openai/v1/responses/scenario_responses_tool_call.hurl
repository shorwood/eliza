POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-1966",
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
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 10
jsonpath "$.id" startsWith "resp_"
jsonpath "$.object" == "response"
jsonpath "$.status" == "completed"
jsonpath "$.error" == null
jsonpath "$.incomplete_details" == null
jsonpath "$.model" == "eliza-1966"
jsonpath "$.output" count == 1
jsonpath "$.output[0].type" == "function_call"
jsonpath "$.output[0].id" startsWith "fc_"
jsonpath "$.output[0].call_id" startsWith "call_"
jsonpath "$.output[0].name" == "echo"
jsonpath "$.output[0].arguments" == "{\"value\":\"hello\"}"
jsonpath "$.output[0].status" == "completed"
jsonpath "$.output_text" == ""
jsonpath "$.usage.input_tokens" isInteger
jsonpath "$.usage.output_tokens" isInteger
jsonpath "$.usage.total_tokens" isInteger
