POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{"model": "eliza-doctor", "stream": true, "input": "Hello"}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "\"type\":\"response.created\""
body contains "\"object\":\"response\""
body contains "\"status\":\"in_progress\""
body contains "\"model\":\"eliza-doctor\""
body contains "\"type\":\"response.output_item.added\""
body contains "\"type\":\"response.output_text.delta\""
body contains "\"type\":\"response.output_text.done\""
body contains "\"type\":\"response.output_item.done\""
body contains "\"type\":\"response.completed\""
body contains "\"status\":\"completed\""
