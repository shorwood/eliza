POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{"model": "eliza-1966", "think": "", "messages": [{"role": "user", "content": "Hello"}]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::ollama::invalid_think"
