POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{"model": "eliza-1966", "max_tokens": 2048, "stream": true, "thinking": {"type": "adaptive"}, "messages": [{"role": "user", "content": "I am sad"}]}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "\"index\":0,\"content_block\":{\"type\":\"thinking\""
body contains "\"type\":\"thinking_delta\""
body contains "\"type\":\"signature_delta\""
body contains "\"index\":1,\"content_block\":{\"type\":\"text\""
body contains "\"output_tokens\":23"
