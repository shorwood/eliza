POST {{base_url}}/openai/v1/audio/speech
Content-Type: application/json
```raw
{"model":
```
HTTP 401
[Asserts]
header "x-eliza-error-code" == "eliza::auth::failed"
jsonpath "$.error.type" == "authentication_error"

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
