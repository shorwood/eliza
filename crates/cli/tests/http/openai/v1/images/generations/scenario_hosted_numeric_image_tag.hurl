POST {{base_url}}/openai/v1/images/generations
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.36
Content-Type: application/json
{"model":"eliza-retro-image","prompt":"Hello","size":"256x256"}
HTTP 429
