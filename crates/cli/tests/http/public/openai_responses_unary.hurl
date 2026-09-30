POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-1966",
  "input": "I need help"
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 10
jsonpath "$.id" startsWith "resp_"
jsonpath "$.object" == "response"
jsonpath "$.created_at" isInteger
jsonpath "$.status" == "completed"
jsonpath "$.error" == null
jsonpath "$.incomplete_details" == null
jsonpath "$.model" == "eliza-1966"
jsonpath "$.output" count == 1
jsonpath "$.output[0].content[0].type" == "output_text"
jsonpath "$.output[0].type" == "message"
jsonpath "$.output[0].id" startsWith "msg_"
jsonpath "$.output[0].status" == "completed"
jsonpath "$.output[0].role" == "assistant"
jsonpath "$.output[0].content" count == 1
jsonpath "$.output[0].content[0].text" == "WHAT WOULD IT MEAN TO YOU IF YOU GOT HELP"
jsonpath "$.output[0].content[0].annotations" count == 0
jsonpath "$.output[0].content[0].logprobs" count == 0
jsonpath "$.output_text" == "WHAT WOULD IT MEAN TO YOU IF YOU GOT HELP"
jsonpath "$.usage.input_tokens" == 3
jsonpath "$.usage.input_tokens_details.cached_tokens" == 0
jsonpath "$.usage.output_tokens" == 10
jsonpath "$.usage.output_tokens_details.reasoning_tokens" == 0
jsonpath "$.usage.total_tokens" == 13
