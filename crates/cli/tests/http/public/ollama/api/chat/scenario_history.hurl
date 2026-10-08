POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": false,
  "messages": [
    {"role": "assistant", "content": "", "thinking": "prior trace"},
    {"role": "user", "content": "Bananas"}
  ]
}
HTTP 200
