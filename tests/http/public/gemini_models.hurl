GET {{base_url}}/gemini/v1beta/models
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$" isObject
jsonpath "$.models" count == 3
jsonpath "$.models[0].*" count == 7
jsonpath "$.models[0].name" == "models/eliza-1966"
jsonpath "$.models[0].version" == "1966-doctor"
jsonpath "$.models[0].displayName" == "ELIZA DOCTOR"
jsonpath "$.models[0].description" == "Classic ELIZA DOCTOR script served through Gemini-compatible JSON."
jsonpath "$.models[0].supportedGenerationMethods" count == 2
jsonpath "$.models[0].supportedGenerationMethods[0]" == "generateContent"
jsonpath "$.models[0].supportedGenerationMethods[1]" == "streamGenerateContent"
jsonpath "$.models[0].inputTokenLimit" == 8000
jsonpath "$.models[0].outputTokenLimit" == 512
jsonpath "$.models[1].name" == "models/flite"
jsonpath "$.models[1].version" == "flite-kal-8khz"
jsonpath "$.models[1].displayName" == "ELIZA Retro TTS"
jsonpath "$.models[1].supportedGenerationMethods" count == 2
jsonpath "$.models[1].inputTokenLimit" == 8000
jsonpath "$.models[1].outputTokenLimit" == 6000
jsonpath "$.models[2].name" == "models/fnv-embed"
jsonpath "$.models[2].version" == "fnv1a-v1"
jsonpath "$.models[2].displayName" == "FNV Embed"
jsonpath "$.models[2].description" == "Deterministic FNV-1a feature-hashed text embeddings for compatibility testing."
jsonpath "$.models[2].supportedGenerationMethods" count == 2
jsonpath "$.models[2].supportedGenerationMethods[0]" == "embedContent"
jsonpath "$.models[2].supportedGenerationMethods[1]" == "batchEmbedContents"
jsonpath "$.models[2].inputTokenLimit" == 8000
jsonpath "$.models[2].outputTokenLimit" == 1024
