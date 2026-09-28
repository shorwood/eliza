GET {{base_url}}/openapi.json
HTTP 200
[Asserts]
jsonpath "$.paths.*" count == 11
jsonpath "$.paths['/openai/v1/chat/completions']" exists
jsonpath "$.paths['/openai/v1/responses']" exists
jsonpath "$.paths['/openai/v1/models']" exists
jsonpath "$.paths['/anthropic/v1/messages']" exists
jsonpath "$.paths['/anthropic/v1/models']" exists
jsonpath "$.paths['/gemini/v1beta/models']" exists
jsonpath "$.paths['/gemini/v1beta/models/{model_action}']" exists
jsonpath "$.paths['/gemini/v1beta/openai/chat/completions']" exists
jsonpath "$.paths['/ollama/api/chat']" exists
jsonpath "$.paths['/ollama/api/tags']" exists
jsonpath "$.paths['/healthz']" exists
jsonpath "$.paths['/openai/v1/chat/completions'].post.requestBody" isObject
jsonpath "$.paths['/anthropic/v1/messages'].post.requestBody" isObject
jsonpath "$.paths['/gemini/v1beta/models/{model_action}'].post.requestBody" isObject
jsonpath "$.components.schemas" isObject
