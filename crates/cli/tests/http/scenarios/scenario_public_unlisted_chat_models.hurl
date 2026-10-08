# OpenAI Chat Completions rejects an unlisted chat model.
POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{"model":"missing-chat","messages":[{"role":"user","content":"Hello"}]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::chat::model_required"
jsonpath "$.error.param" == "model"

# OpenAI Responses uses the same configured chat names.
POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{"model":"missing-chat","input":"Hello"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::chat::model_required"
jsonpath "$.error.param" == "model"

# The Gemini OpenAI alias shares Chat Completions model validation.
POST {{base_url}}/gemini/v1beta/openai/chat/completions
Content-Type: application/json
{"model":"missing-chat","messages":[{"role":"user","content":"Hello"}]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::chat::model_required"
jsonpath "$.error.param" == "model"

# Anthropic Messages rejects an unlisted chat model.
POST {{base_url}}/anthropic/v1/messages
Content-Type: application/json
{"model":"missing-chat","max_tokens":64,"messages":[{"role":"user","content":"Hello"}]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::chat::model_required"
jsonpath "$.error.type" == "invalid_request_error"

# Native Gemini text generation rejects an unlisted chat model.
POST {{base_url}}/gemini/v1beta/models/missing-chat:generateContent
Content-Type: application/json
{"contents":[{"role":"user","parts":[{"text":"Hello"}]}]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::chat::model_required"
jsonpath "$.error.status" == "INVALID_ARGUMENT"

# Ollama chat rejects an unlisted chat model.
POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{"model":"missing-chat","stream":false,"messages":[{"role":"user","content":"Hello"}]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::chat::model_required"
jsonpath "$.error" contains "eliza-1966"
