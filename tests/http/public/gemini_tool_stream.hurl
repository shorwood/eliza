POST {{base_url}}/gemini/v1beta/models/eliza-doctor:streamGenerateContent?alt=sse
Content-Type: application/json
{
  "tools": [{"functionDeclarations": [{
    "name": "echo",
    "parameters": {"type": "object"}
  }]}],
  "contents": [{"role": "user", "parts": [{
    "text": "@tool echo {\"value\":\"hello\"}"
  }]}]
}
HTTP 200
[Asserts]
body contains "functionCall"
body contains "echo"
