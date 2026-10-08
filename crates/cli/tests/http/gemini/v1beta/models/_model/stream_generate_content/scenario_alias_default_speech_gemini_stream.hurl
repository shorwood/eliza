POST {{base_url}}/gemini/v1beta/models/voice-two:streamGenerateContent?alt=sse
x-goog-api-key: {{token}}
Content-Type: application/json
{"contents":[{"role":"user","parts":[{"text":"Hello"}]}],"generationConfig":{
  "responseModalities":["AUDIO"],
  "speechConfig":{"voiceConfig":{"voice":"Kore"}}
}}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "voice-two"
