POST {{base_url}}/gemini/v1beta/models/flite:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "Hello"}]}],
  "generationConfig": {
    "responseModalities": ["AUDIO"],
    "speechConfig": {"voiceConfig": {"voice": "Kore"}},
    "responseFormat": {"audio": {"mimeType": "AUDIO_ALAW", "sampleRate": 8000}}
  }
}
HTTP 200
[Asserts]
jsonpath "$.candidates[0].content.parts[0].inlineData.mimeType" == "audio/alaw;rate=8000"
