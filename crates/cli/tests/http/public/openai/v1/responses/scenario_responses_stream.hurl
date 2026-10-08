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
