POST {{base_url}}/openai/v1/audio/speech
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "model": "flite",
  "input": "Authorized robot",
  "voice": "Kore",
  "response_format": "pcm"
}
HTTP 200
[Asserts]
header "Content-Type" == "audio/pcm;rate=24000"
