POST {{base_url}}/openai/v1/embeddings
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.37
Content-Type: application/json
["fnv-embed", ["Hello"], 8, null, null]
HTTP 400
[Asserts]
body not contains "$3"
