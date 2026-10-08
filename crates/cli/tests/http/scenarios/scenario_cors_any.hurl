# Any-origin mode permits browser preflight from an arbitrary origin.
OPTIONS {{base_url}}/openai/v1/chat/completions
Origin: https://unlisted.example.test
Access-Control-Request-Method: POST
Access-Control-Request-Headers: authorization,content-type
HTTP 200
[Asserts]
header "Access-Control-Allow-Origin" == "*"
