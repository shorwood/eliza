POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{"model": "eliza-1966", "input": "I am sad", "stream": true, "reasoning": {"summary": "concise"}}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "\"type\":\"response.reasoning_summary_part.added\""
body contains "\"type\":\"response.reasoning_summary_text.delta\""
body contains "\"type\":\"response.reasoning_summary_text.done\""
body contains "\"output_index\":0,\"item\":{\"type\":\"reasoning\""
body contains "\"output_index\":1,\"item\":{\"type\":\"message\""
body contains "\"reasoning_tokens\":15"

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

POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{"model": "eliza-1966", "think": true, "messages": [{"role": "user", "content": "I am sad"}]}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/x-ndjson"
body contains "\"content\":\"\",\"thinking\":\"NORMALIZED "
body contains "\"content\":\"I "
body contains "\"eval_count\":23"
