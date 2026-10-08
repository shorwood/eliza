# The listed development origin can preflight an authenticated OpenAI request.
OPTIONS {{base_url}}/openai/v1/chat/completions
Origin: http://localhost:3000
Access-Control-Request-Method: POST
Access-Control-Request-Headers: authorization,content-type
HTTP 200
[Asserts]
header "Access-Control-Allow-Origin" == "http://localhost:3000"

# A second listed origin can preflight the native Gemini API key header.
OPTIONS {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Origin: https://app.example.test
Access-Control-Request-Method: POST
Access-Control-Request-Headers: x-goog-api-key,content-type
HTTP 200
[Asserts]
header "Access-Control-Allow-Origin" == "https://app.example.test"

# An unlisted origin does not receive browser permission.
OPTIONS {{base_url}}/openai/v1/chat/completions
Origin: https://unlisted.example.test
Access-Control-Request-Method: POST
Access-Control-Request-Headers: authorization,content-type
HTTP 200
[Asserts]
header "Access-Control-Allow-Origin" not exists

# Allowed origins receive CORS headers on authenticated responses too.
POST {{base_url}}/openai/v1/chat/completions
Origin: http://localhost:3000
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"eliza-1966","messages":[{"role":"user","content":"Hello"}]}
HTTP 200
[Asserts]
header "Access-Control-Allow-Origin" == "http://localhost:3000"

# The API can still answer an unlisted origin, but a browser cannot read it.
POST {{base_url}}/openai/v1/chat/completions
Origin: https://unlisted.example.test
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"eliza-1966","messages":[{"role":"user","content":"Hello"}]}
HTTP 200
[Asserts]
header "Access-Control-Allow-Origin" not exists
