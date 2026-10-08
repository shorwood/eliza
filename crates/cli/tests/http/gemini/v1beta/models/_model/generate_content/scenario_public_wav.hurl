POST {{base_url}}/gemini/v1beta/models/flite:generateContent
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
jsonpath "$.modelVersion" == "flite"
jsonpath "$.usageMetadata.promptTokenCount" == 2
jsonpath "$.usageMetadata.candidatesTokenCount" > 0
