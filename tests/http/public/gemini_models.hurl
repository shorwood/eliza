GET {{base_url}}/gemini/v1beta/models
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$" isObject
jsonpath "$.models" count == 2
jsonpath "$.models[0].*" count == 7
jsonpath "$.models[0].name" == "models/eliza-doctor"
jsonpath "$.models[0].version" == "1966-doctor"
jsonpath "$.models[0].displayName" == "ELIZA DOCTOR"
jsonpath "$.models[0].description" == "Classic ELIZA DOCTOR script served through Gemini-compatible JSON."
jsonpath "$.models[0].supportedGenerationMethods" count == 2
jsonpath "$.models[0].supportedGenerationMethods[0]" == "generateContent"
jsonpath "$.models[0].supportedGenerationMethods[1]" == "streamGenerateContent"
jsonpath "$.models[0].inputTokenLimit" == 8000
jsonpath "$.models[0].outputTokenLimit" == 512
jsonpath "$.models[1].name" == "models/eliza-retro-tts"
jsonpath "$.models[1].version" == "flite-kal-8khz"
jsonpath "$.models[1].displayName" == "ELIZA Retro TTS"
jsonpath "$.models[1].supportedGenerationMethods" count == 2
jsonpath "$.models[1].inputTokenLimit" == 8000
jsonpath "$.models[1].outputTokenLimit" == 6000
