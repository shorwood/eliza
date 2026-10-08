POST {{base_url}}/openai/v1/chat/completions
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.33
Content-Type: application/json
{"model":"eliza-1966","messages":[{"role":"user","content":[{"type":"text","text":"Hello"},{"type":"image_url","image_url":{"url":"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABAQAAAADcWUInAAAACklEQVQI12NwAAAAQgBBg7nsrQAAAABJRU5ErkJggg=="}}]}]}
HTTP 200

POST {{base_url}}/openai/v1/images/generations
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.33
Content-Type: application/json
{"model":"eliza-retro-image","prompt":"Hello","size":"256x256"}
HTTP 429
[Asserts]
body contains "$3"
