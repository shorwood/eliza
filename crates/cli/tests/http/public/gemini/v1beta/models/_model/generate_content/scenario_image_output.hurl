POST {{base_url}}/gemini/v1beta/models/flite:generateContent
Content-Type: application/json
{"contents": [{"role": "user", "parts": [{"text": "Hello"}]}]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::speech_model_audio_only"
