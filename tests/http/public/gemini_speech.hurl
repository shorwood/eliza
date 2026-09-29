POST {{base_url}}/gemini/v1beta/models/eliza-retro-tts:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "Hello world"}]}],
  "generationConfig": {
    "responseModalities": ["AUDIO"],
    "speechConfig": {
      "voiceConfig": {"voice": "Kore"}
    },
    "responseFormat": {"audio": {"mimeType": "AUDIO_WAV"}}
  }
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.candidates[0].content.parts[0].inlineData.mimeType" == "audio/wav"
jsonpath "$.candidates[0].content.parts[0].inlineData.data" startsWith "UklGR"
jsonpath "$.candidates[0].finishReason" == "STOP"
jsonpath "$.modelVersion" == "eliza-retro-tts"
jsonpath "$.usageMetadata.promptTokenCount" == 2
jsonpath "$.usageMetadata.candidatesTokenCount" > 0

POST {{base_url}}/gemini/v1beta/models/eliza-retro-tts:generateContent
Content-Type: application/json
{
  "contents": [{
    "role": "user",
    "parts": [
      {"text": "Hello Jane.", "speech_metadata": {"speaker": "Joe", "style": "low"}},
      {"text": "Hello Joe.", "speechMetadata": {"speaker": "Jane", "style": "cheerful"}}
    ]
  }],
  "generationConfig": {
    "responseModalities": ["AUDIO"],
    "speechConfig": {
      "multiSpeakerVoiceConfig": {
        "speakerVoiceConfigs": [
          {"speaker": "Joe", "voiceConfig": {"prebuiltVoiceConfig": {"voiceName": "Puck"}}},
          {"speaker": "Jane", "voiceConfig": {"prebuiltVoiceConfig": {"voiceName": "Kore"}}}
        ]
      }
    },
    "responseFormat": {"audio": {"mimeType": "AUDIO_MULAW", "sampleRate": 8000}}
  }
}
HTTP 200
[Asserts]
jsonpath "$.candidates[0].content.parts[0].inlineData.mimeType" == "audio/mulaw;rate=8000"
jsonpath "$.candidates[0].content.parts[0].inlineData.data" isString

POST {{base_url}}/gemini/v1beta/models/eliza-retro-tts:streamGenerateContent?alt=sse
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "Streaming robot"}]}],
  "generationConfig": {
    "responseModalities": ["AUDIO"],
    "speechConfig": {"voiceConfig": {"voice": "retro"}}
  }
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "\"mimeType\":\"audio/L16;rate=24000\""
body contains "\"finishReason\":\"STOP\""
body contains "\"usageMetadata\""

POST {{base_url}}/gemini/v1beta/models/eliza-retro-tts:streamGenerateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "Streaming JSON robot"}]}],
  "generationConfig": {
    "responseModalities": ["AUDIO"],
    "speechConfig": {"voiceConfig": {"voice": "retro"}}
  }
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$[0].candidates[0].content.parts[0].inlineData.mimeType" == "audio/L16;rate=24000"
body contains "\"finishReason\":\"STOP\""
body contains "\"usageMetadata\""

POST {{base_url}}/gemini/v1beta/models/eliza-doctor:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "Hello"}]}],
  "generationConfig": {
    "responseModalities": ["AUDIO"],
    "speechConfig": {"voiceConfig": {"voice": "retro"}}
  }
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::speech_model_required"

POST {{base_url}}/gemini/v1beta/models/eliza-retro-tts:generateContent
Content-Type: application/json
{"contents": [{"role": "user", "parts": [{"text": "Hello"}]}]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::speech_model_audio_only"
