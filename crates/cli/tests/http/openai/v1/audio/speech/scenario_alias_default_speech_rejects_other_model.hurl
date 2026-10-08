POST {{base_url}}/openai/v1/audio/speech
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"flite","input":"Hello","voice":"Kore"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::speech_model_required"
jsonpath "$.error.message" contains "voice-one"
