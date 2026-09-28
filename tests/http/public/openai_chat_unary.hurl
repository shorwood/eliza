POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-doctor",
  "messages": [{"role": "user", "content": "I am sad"}],
  "future_field": true
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 6
jsonpath "$.id" startsWith "chatcmpl-"
jsonpath "$.object" == "chat.completion"
jsonpath "$.created" isInteger
jsonpath "$.model" == "eliza-doctor"
jsonpath "$.choices" count == 1
jsonpath "$.choices[0].index" == 0
jsonpath "$.choices[0].message.role" == "assistant"
jsonpath "$.choices[0].message.content" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.choices[0].message.tool_calls" not exists
jsonpath "$.choices[0].finish_reason" == "stop"
jsonpath "$.usage.prompt_tokens" == 3
jsonpath "$.usage.completion_tokens" == 8
jsonpath "$.usage.total_tokens" == 11
