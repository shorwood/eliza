# Catalog aliases retain authentication, order, defaults, and deduplication.
GET {{base_url}}/openai/v1/models
HTTP 401

GET {{base_url}}/openai/v1/models
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 9
jsonpath "$.data[0].id" == "chat-one"
jsonpath "$.data[1].id" == "chat-two"
jsonpath "$.data[2].id" == "voice-one"
jsonpath "$.data[3].id" == "voice-two"
jsonpath "$.data[4].id" == "embed-one"
jsonpath "$.data[5].id" == "embed-two"
jsonpath "$.data[6].id" == "embed{three}"
jsonpath "$.data[7].id" == "image-one"
jsonpath "$.data[8].id" == "image-two"

GET {{base_url}}/anthropic/v1/models
x-api-key: {{token}}
HTTP 200
[Asserts]
jsonpath "$.data" count == 2
jsonpath "$.data[0].id" == "chat-one"
jsonpath "$.data[1].id" == "chat-two"
jsonpath "$.first_id" == "chat-one"
jsonpath "$.last_id" == "chat-two"
jsonpath "$.has_more" == false

GET {{base_url}}/gemini/v1beta/models
x-goog-api-key: {{token}}
HTTP 200
[Asserts]
jsonpath "$.models" count == 9
jsonpath "$.models[0].name" == "models/chat-one"
jsonpath "$.models[1].name" == "models/chat-two"
jsonpath "$.models[2].name" == "models/voice-one"
jsonpath "$.models[3].name" == "models/voice-two"
jsonpath "$.models[4].name" == "models/embed-one"
jsonpath "$.models[4].supportedGenerationMethods[0]" == "embedContent"
jsonpath "$.models[5].name" == "models/embed-two"
jsonpath "$.models[6].name" == "models/embed{three}"
jsonpath "$.models[7].name" == "models/image-one"
jsonpath "$.models[8].name" == "models/image-two"

GET {{base_url}}/ollama/api/tags
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.models" count == 5
jsonpath "$.models[0].name" == "chat-one"
jsonpath "$.models[1].name" == "chat-two"
jsonpath "$.models[2].name" == "embed-one"
jsonpath "$.models[2].details.family" == "eliza-embed"
jsonpath "$.models[3].name" == "embed-two"
jsonpath "$.models[4].name" == "embed{three}"

# Chat preserves requested IDs in unary and streaming responses.
POST {{base_url}}/openai/v1/chat/completions
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"chat-one","messages":[{"role":"user","content":"I need help"}]}
HTTP 200
[Asserts]
jsonpath "$.model" == "chat-one"
jsonpath "$.choices[0].message.content" != ""

POST {{base_url}}/openai/v1/chat/completions
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"chat-two","stream":true,"messages":[{"role":"user","content":"I need help"}]}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "chat-two"
body contains "[DONE]"

POST {{base_url}}/anthropic/v1/messages
x-api-key: {{token}}
Content-Type: application/json
{"model":"chat-two","max_tokens":128,"messages":[{"role":"user","content":"I need help"}]}
HTTP 200
[Asserts]
jsonpath "$.model" == "chat-two"

POST {{base_url}}/ollama/api/chat
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"chat-one","stream":false,"messages":[{"role":"user","content":"I need help"}]}
HTTP 200
[Asserts]
jsonpath "$.model" == "chat-one"

POST {{base_url}}/gemini/v1beta/models/chat-two:generateContent
x-goog-api-key: {{token}}
Content-Type: application/json
{"contents":[{"role":"user","parts":[{"text":"I need help"}]}]}
HTTP 200
[Asserts]
jsonpath "$.modelVersion" == "chat-two"

# Embedding aliases still produce the deterministic vectors.
POST {{base_url}}/openai/v1/embeddings
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"embed-two","input":"Hello world","dimensions":8}
HTTP 200
[Asserts]
jsonpath "$.model" == "embed-two"
jsonpath "$.data[0].embedding" count == 8
jsonpath "$.data[0].embedding[1]" == -0.48507127

POST {{base_url}}/ollama/api/embed
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"embed-one","input":"Hello world","dimensions":8}
HTTP 200
[Asserts]
jsonpath "$.model" == "embed-one"
jsonpath "$.embeddings[0][1]" == -0.48507127

POST {{base_url}}/gemini/v1beta/models/embed%7Bthree%7D:embedContent
x-goog-api-key: {{token}}
Content-Type: application/json
{"model":"models/embed{three}","content":{"parts":[{"text":"Hello world"}]},"outputDimensionality":8}
HTTP 200
[Asserts]
jsonpath "$.embedding.values" count == 8
jsonpath "$.embedding.values[1]" == -0.48507127

POST {{base_url}}/gemini/v1beta/models/embed-two:batchEmbedContents
x-goog-api-key: {{token}}
Content-Type: application/json
{"requests":[
  {"model":"models/embed-two","content":{"parts":[{"text":"Hello world"}]},"outputDimensionality":8},
  {"model":"models/embed-two","content":{"parts":[{"text":"Another text"}]},"outputDimensionality":8}
]}
HTTP 200
[Asserts]
jsonpath "$.embeddings" count == 2
jsonpath "$.embeddings[0].values[1]" == -0.48507127

POST {{base_url}}/gemini/v1beta/models/embed-one:embedContent
x-goog-api-key: {{token}}
Content-Type: application/json
{"model":"models/embed-two","content":{"parts":[{"text":"Hello world"}]}}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::embedding_model_required"
jsonpath "$.error.message" contains "models/embed-one"

POST {{base_url}}/gemini/v1beta/models/embed-one:batchEmbedContents
x-goog-api-key: {{token}}
Content-Type: application/json
{"requests":[
  {"model":"models/embed-one","content":{"parts":[{"text":"Hello world"}]}},
  {"model":"models/embed-two","content":{"parts":[{"text":"Another text"}]}}
]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::embedding_model_required"

POST {{base_url}}/openai/v1/embeddings
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"fnv-embed","input":"Hello world"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::embedding::model_required"
jsonpath "$.error.message" contains "embed-one"

# Speech aliases work for raw audio and both streaming surfaces.
POST {{base_url}}/openai/v1/audio/speech
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"voice-one","input":"Hello","voice":"Kore","response_format":"pcm"}
HTTP 200
[Asserts]
header "Content-Type" == "audio/pcm;rate=24000"

POST {{base_url}}/openai/v1/audio/speech
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"voice-two","input":"Hello","voice":"Kore","response_format":"pcm","stream_format":"sse"}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "event: speech.audio.done"

POST {{base_url}}/gemini/v1beta/models/voice-two:streamGenerateContent?alt=sse
x-goog-api-key: {{token}}
Content-Type: application/json
{"contents":[{"role":"user","parts":[{"text":"Hello"}]}],"generationConfig":{
  "responseModalities":["AUDIO"],
  "speechConfig":{"voiceConfig":{"voice":"Kore"}}
}}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "voice-two"

POST {{base_url}}/gemini/v1beta/models/voice-one:generateContent
x-goog-api-key: {{token}}
Content-Type: application/json
{"contents":[{"role":"user","parts":[{"text":"Hello"}]}]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::speech_model_audio_only"
jsonpath "$.error.message" contains "voice-one"

POST {{base_url}}/openai/v1/audio/speech
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"flite","input":"Hello","voice":"Kore"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::speech_model_required"
jsonpath "$.error.message" contains "voice-one"

# Image aliases include omitted-model defaults and Gemini's inferred mode.
POST {{base_url}}/openai/v1/images/generations
Authorization: Bearer {{token}}
Content-Type: application/json
{"prompt":"retro cats","size":"256x256"}
HTTP 200
[Asserts]
jsonpath "$.data[0].b64_json" startsWith "iVBOR"

POST {{base_url}}/openai/v1/images/generations
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"image-two","prompt":"retro cats","size":"256x256"}
HTTP 200
[Asserts]
jsonpath "$.data[0].b64_json" startsWith "iVBOR"

POST {{base_url}}/gemini/v1beta/models/image-one:generateContent
x-goog-api-key: {{token}}
Content-Type: application/json
{"contents":[{"role":"user","parts":[{"text":"retro cats"}]}]}
HTTP 200
[Asserts]
jsonpath "$.modelVersion" == "image-one"
jsonpath "$.candidates[0].content.parts[1].inlineData.mimeType" == "image/png"

POST {{base_url}}/gemini/v1beta/models/image-two:generateContent
x-goog-api-key: {{token}}
Content-Type: application/json
{"contents":[{"role":"user","parts":[{"text":"retro cats"}]}],"generationConfig":{"responseModalities":["TEXT"]}}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::image_model_requires_image"
jsonpath "$.error.message" contains "image-two"

POST {{base_url}}/openai/v1/images/generations
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"eliza-retro-image","prompt":"retro cats","size":"256x256"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::image_generation_model_required"
jsonpath "$.error.message" contains "image-one"
