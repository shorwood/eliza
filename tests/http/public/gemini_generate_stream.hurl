POST {{base_url}}/gemini/v1beta/models/eliza-doctor:streamGenerateContent?alt=sse
Content-Type: application/json
{"contents": [{"role": "user", "parts": [{"text": "Hello"}]}]}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "data: {\"candidates\""
body contains "\"role\":\"model\""
body contains "\"modelVersion\":\"eliza-doctor\""
body contains "\"finishReason\":\"STOP\""
body contains "\"usageMetadata\""
body contains "\"promptTokenCount\""
body contains "\"candidatesTokenCount\""
body contains "\"totalTokenCount\""
