POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
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
