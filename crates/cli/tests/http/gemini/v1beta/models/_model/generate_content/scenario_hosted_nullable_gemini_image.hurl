POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.37
Content-Type: application/json
{"contents":[{"parts":[{"text":"Hello"}]}],"generationConfig":[["AUDIO"],null,null,null,null]}
HTTP 400
[Asserts]
body not contains "$3"
