POST {{base_url}}/openai/v1/audio/speech
Content-Type: application/json
{
  "model": "flite",
  "input": "Hello",
  "voice": "Kore",
  "response_format": "flac"
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::unsupported_speech_format"
jsonpath "$.error.param" == "response_format"
