GET {{base_url}}/openapi.json
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.openapi" == "3.1.0"
jsonpath "$.info.title" == "ELIZA Compatibility Server"
jsonpath "$.info.version" == "0.1.0"
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
jsonpath "$.paths['/ollama/api/chat'].post.requestBody" isObject
jsonpath "$.components.schemas.ChatCompletionRequest" isObject
jsonpath "$.components.schemas.MessagesRequest" isObject
jsonpath "$.components.schemas.GenerateContentRequest" isObject
jsonpath "$.components.schemas.ChatRequest" isObject
jsonpath "$.components.schemas.OpenAiFailureResponse" isObject
jsonpath "$.components.schemas.AnthropicFailureResponse" isObject
jsonpath "$.components.schemas.GeminiFailureResponse" isObject
jsonpath "$.components.schemas.OllamaFailureResponse" isObject
