POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{"model": "eliza-doctor", "stream": true, "input": "Hello"}
HTTP 200
[Asserts]
body contains "response.output_text.delta"
body contains "response.completed"
