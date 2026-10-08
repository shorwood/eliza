POST {{base_url}}/openai/v1/audio/speech
Content-Type: application/json
{"model": "eliza-1966", "input": "Hello", "voice": "Kore"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::speech_model_required"
jsonpath "$.error.param" == "model"
