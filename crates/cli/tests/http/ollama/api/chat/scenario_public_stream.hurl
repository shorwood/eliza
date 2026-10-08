POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{"model": "eliza-1966", "think": true, "messages": [{"role": "user", "content": "I am sad"}]}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/x-ndjson"
body contains "\"content\":\"\",\"thinking\":\"NORMALIZED "
body contains "\"content\":\"I "
body contains "\"eval_count\":23"
