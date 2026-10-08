POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{
  "model": "eliza-1966",
  "max_tokens": 128,
  "messages": [
    {"role": "assistant", "content": [{"type": "thinking", "thinking": "prior trace", "signature": "hJ1c6IwuPTM="}]},
    {"role": "user", "content": "Bananas"}
  ]
}
HTTP 200
