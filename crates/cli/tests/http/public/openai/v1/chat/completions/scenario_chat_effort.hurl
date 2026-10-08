# Chat surfaces accept effort without adding a nonstandard output extension.
POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "reasoning_effort": "high",
  "messages": [{"role": "user", "content": "I am sad"}]
}
HTTP 200
[Asserts]
jsonpath "$.choices[0].message.content" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.choices[0].message.reasoning_content" not exists
jsonpath "$.usage.total_tokens" == 11
