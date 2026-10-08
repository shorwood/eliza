POST {{base_url}}/gemini/v1beta/models/flite:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "Hello"}]}],
  "generationConfig": {
    "responseModalities": ["AUDIO"],
    "responseMimeType": "application/json",
    "speechConfig": {"voiceConfig": {"voice": "retro"}}
  }
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::text_format_for_audio"
jsonpath "$.error.status" == "INVALID_ARGUMENT"
