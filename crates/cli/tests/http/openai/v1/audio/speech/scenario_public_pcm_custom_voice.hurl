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
