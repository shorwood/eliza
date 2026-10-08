POST {{base_url}}/openai/v1/chat/completions
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.34
Content-Type: application/json
{"model":"eliza-1966","messages":[{"role":"user","content":"Hello"}],"tools":[{"type":"function","function":{"name":"inspect","parameters":{"type":"object","properties":{"inlineData":{"type":"string"}}}}}]}
HTTP 200

POST {{base_url}}/openai/v1/images/generations
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.34
Content-Type: application/json
{"model":"eliza-retro-image","prompt":"Hello","size":"256x256"}
HTTP 200
