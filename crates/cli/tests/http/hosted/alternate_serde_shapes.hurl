POST {{base_url}}/openai/v1/chat/completions
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.36
Content-Type: application/json
{"model":"eliza-1966","messages":[{"role":"user","content":[{"type":1,"image_url":{"url":"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABAQAAAADcWUInAAAACklEQVQI12NwAAAAQgBBg7nsrQAAAABJRU5ErkJggg=="}}]}]}
HTTP 200

POST {{base_url}}/openai/v1/images/generations
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.36
Content-Type: application/json
{"model":"eliza-retro-image","prompt":"Hello","size":"256x256"}
HTTP 429

POST {{base_url}}/openai/v1/embeddings
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.37
Content-Type: application/json
["fnv-embed", ["Hello"], 8, null, null]
HTTP 400
[Asserts]
body not contains "$5"

POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.37
Content-Type: application/json
{"contents":[{"parts":[{"text":"Hello"}]}],"generationConfig":[["AUDIO"],null,null,null,null]}
HTTP 400
[Asserts]
body not contains "$5"
