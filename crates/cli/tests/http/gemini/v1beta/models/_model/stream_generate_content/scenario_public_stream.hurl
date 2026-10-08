POST {{base_url}}/gemini/v1beta/models/eliza-1966:streamGenerateContent?alt=sse
Content-Type: application/json
{"contents": [{"parts": [{"text": "I am sad"}]}], "generationConfig": {"thinkingConfig": {"includeThoughts": true}}}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "\"thought\":true"
body contains "\"thoughtSignature\":\"hJ1c6IwuPTM=\""
body contains "\"text\":\"I "
body contains "\"thoughtsTokenCount\":15"
