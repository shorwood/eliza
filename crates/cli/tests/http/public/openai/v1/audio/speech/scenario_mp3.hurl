POST {{base_url}}/openai/v1/audio/speech
Content-Type: application/json
{"model":"flite","input":"Hello","voice":"Kore","response_format":"mp3"}
HTTP 200
[Asserts]
header "Content-Type" == "audio/mpeg"
