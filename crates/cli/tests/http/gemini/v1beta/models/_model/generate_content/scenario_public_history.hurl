POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{
  "contents": [
    {"role": "model", "parts": [{"text": "prior trace", "thought": true, "thoughtSignature": "hJ1c6IwuPTM="}]},
    {"role": "user", "parts": [{"text": "Bananas"}]}
  ]
}
HTTP 200
