POST {{base_url}}/openai/v1/audio/speech
Content-Type: application/json
{
  "model": "flite",
  "input": "Hello from the nineties.",
  "voice": "Kore",
  "instructions": "calm and low",
  "response_format": "wav"
}
HTTP 200
[Asserts]
header "Content-Type" == "audio/wav"
header "Transfer-Encoding" == "chunked"
