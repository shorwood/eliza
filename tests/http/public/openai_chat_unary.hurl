POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-doctor",
  "messages": [{"role": "user", "content": "I am sad"}],
  "future_field": true
}
HTTP 200
[Asserts]
jsonpath "$.object" == "chat.completion"
jsonpath "$.choices[0].message.content" == "I AM SORRY TO HEAR YOU ARE SAD"
