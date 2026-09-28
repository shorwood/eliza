POST {{base_url}}/gemini/v1beta/openai/chat/completions
Content-Type: application/json
{
  "model": "eliza-doctor",
  "messages": [{"role": "user", "content": "Hello"}]
}
HTTP 200
[Asserts]
jsonpath "$.object" == "chat.completion"
