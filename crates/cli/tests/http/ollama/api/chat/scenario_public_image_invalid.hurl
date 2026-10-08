POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{"model": "eliza-1966", "stream": false, "messages": [{"role": "user", "images": ["%%%"]}]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::image::invalid_base64"
jsonpath "$.error" contains "invalid image base64"
