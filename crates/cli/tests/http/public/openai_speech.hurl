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

POST {{base_url}}/openai/v1/audio/speech
Content-Type: application/json
{
  "model": "flite",
  "input": "Hello",
  "voice": {"id": "custom-robot"},
  "response_format": "pcm",
  "stream_format": "sse"
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "event: speech.audio.delta"
body contains "\"type\":\"speech.audio.delta\""
body contains "event: speech.audio.done"
body contains "\"input_tokens\""
body contains "\"output_tokens\""
body contains "\"total_tokens\""

POST {{base_url}}/openai/v1/audio/speech
Content-Type: application/json
{"model": "eliza-1966", "input": "Hello", "voice": "Kore"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::speech_model_required"
jsonpath "$.error.param" == "model"

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
