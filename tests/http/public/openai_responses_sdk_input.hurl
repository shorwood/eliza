POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-doctor",
  "input": [{
    "type": "message",
    "role": "user",
    "content": [{"type": "input_text", "text": "I am sad"}]
  }]
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
jsonpath "$.model" == "eliza-doctor"
jsonpath "$.output" count == 1
jsonpath "$.output[0].type" == "message"
jsonpath "$.output[0].id" startsWith "msg_"
jsonpath "$.output[0].status" == "completed"
jsonpath "$.output[0].role" == "assistant"
jsonpath "$.output[0].content" count == 1
jsonpath "$.output[0].content[0].type" == "output_text"
jsonpath "$.output[0].content[0].text" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.output[0].content[0].annotations" count == 0
jsonpath "$.output[0].content[0].logprobs" count == 0
jsonpath "$.output_text" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.usage.input_tokens" == 3
jsonpath "$.usage.output_tokens" == 8
jsonpath "$.usage.total_tokens" == 11
