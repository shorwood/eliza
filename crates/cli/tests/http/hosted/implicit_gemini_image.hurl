POST {{base_url}}/gemini/v1beta/models/eliza-retro-image:generateContent
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.32
Content-Type: application/json
{"contents":[{"parts":[{"text":"Hello"}]}]}
HTTP 200

POST {{base_url}}/openai/v1/images/generations
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.32
Content-Type: application/json
{"model":"eliza-retro-image","prompt":"Hello","size":"256x256"}
HTTP 429
[Asserts]
header "Retry-After" exists
body contains "$5"
