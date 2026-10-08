POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": false,
  "messages": [{"role": "user", "content": "Hello"}],
  "format": {"$ref": "#"}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::ollama::invalid_response_schema"
body contains "/$ref"
