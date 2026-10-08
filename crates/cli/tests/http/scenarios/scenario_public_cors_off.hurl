# Browser preflight receives no CORS permission by default.
OPTIONS {{base_url}}/openai/v1/chat/completions
Origin: http://localhost:3000
Access-Control-Request-Method: POST
Access-Control-Request-Headers: authorization,content-type
HTTP 405
[Asserts]
header "Access-Control-Allow-Origin" not exists
