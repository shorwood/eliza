POST {{base_url}}/gemini/v1beta/models/eliza-doctor:streamGenerateContent?alt=sse
Content-Type: application/json
{"contents": [{"role": "user", "parts": [{"text": "Hello"}]}]}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "data:"
body contains "finishReason"
