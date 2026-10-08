POST {{base_url}}/openai/v1/audio/speech
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.31
Content-Type: application/json
{"model":"flite","input":"Hello","voice":"Kore","response_format":"pcm"}
HTTP 200

POST {{base_url}}/gemini/v1beta/models/flite:generateContent
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.31
Content-Type: application/json
{"contents":[{"parts":[{"text":"Hello"}]}],"generationConfig":{"responseModalities":["AUDIO"]}}
HTTP 429
[Asserts]
header "Retry-After" exists
body contains "$3"
