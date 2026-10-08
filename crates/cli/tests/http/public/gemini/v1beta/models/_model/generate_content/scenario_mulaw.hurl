POST {{base_url}}/gemini/v1beta/models/flite:generateContent
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
