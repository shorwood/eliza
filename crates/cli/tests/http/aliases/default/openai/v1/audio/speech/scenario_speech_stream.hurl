POST {{base_url}}/openai/v1/audio/speech
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"voice-two","input":"Hello","voice":"Kore","response_format":"pcm","stream_format":"sse"}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "event: speech.audio.done"
