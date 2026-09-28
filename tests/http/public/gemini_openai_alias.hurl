POST {{base_url}}/gemini/v1beta/openai/chat/completions
Content-Type: application/json
{
  "model": "eliza-doctor",
  "messages": [{"role": "user", "content": "Hello"}]
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
jsonpath "$.choices[0].message.content" isString
jsonpath "$.choices[0].message.tool_calls" not exists
jsonpath "$.choices[0].finish_reason" == "stop"
jsonpath "$.usage.prompt_tokens" isInteger
jsonpath "$.usage.completion_tokens" isInteger
jsonpath "$.usage.total_tokens" isInteger
