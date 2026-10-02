GET {{base_url}}/openapi.json
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.openapi" == "3.1.0"
jsonpath "$.info.title" == "ELIZA Compatibility Server"
jsonpath "$.info.version" == "1.0.0"
jsonpath "$.info.license.*" count == 2
jsonpath "$.info.license.name" == "MIT License"
jsonpath "$.info.license.identifier" == "MIT"
jsonpath "$.info.license.url" not exists
jsonpath "$.paths.*" count == 18
jsonpath "$.paths['/openai/v1/chat/completions']" exists
jsonpath "$.paths['/openai/v1/responses']" exists
jsonpath "$.paths['/openai/v1/models']" exists
jsonpath "$.paths['/openai/v1/audio/speech']" exists
jsonpath "$.paths['/openai/v1/embeddings']" exists
jsonpath "$.paths['/openai/v1/images/generations']" exists
jsonpath "$.paths['/anthropic/v1/messages']" exists
jsonpath "$.paths['/anthropic/v1/models']" exists
jsonpath "$.paths['/gemini/v1beta/models']" exists
jsonpath "$.paths['/gemini/v1beta/models/{model_action}']" exists
jsonpath "$.paths['/gemini/v1beta/models/fnv-embed:embedContent']" exists
jsonpath "$.paths['/gemini/v1beta/models/fnv-embed:batchEmbedContents']" exists
jsonpath "$.paths['/gemini/v1beta/openai/chat/completions']" exists
jsonpath "$.paths['/gemini/v1beta/openai/embeddings']" exists
jsonpath "$.paths['/ollama/api/chat']" exists
jsonpath "$.paths['/ollama/api/embed']" exists
jsonpath "$.paths['/ollama/api/tags']" exists
jsonpath "$.paths['/healthz']" exists
jsonpath "$.paths['/openai/v1/chat/completions'].post.requestBody" isObject
jsonpath "$.paths['/openai/v1/audio/speech'].post.requestBody" isObject
jsonpath "$.paths['/openai/v1/embeddings'].post.requestBody" isObject
jsonpath "$.paths['/openai/v1/images/generations'].post.requestBody" isObject
jsonpath "$.paths['/anthropic/v1/messages'].post.requestBody" isObject
jsonpath "$.paths['/gemini/v1beta/models/{model_action}'].post.requestBody" isObject
jsonpath "$.paths['/gemini/v1beta/models/fnv-embed:embedContent'].post.requestBody" isObject
jsonpath "$.paths['/gemini/v1beta/models/fnv-embed:batchEmbedContents'].post.requestBody" isObject
jsonpath "$.paths['/gemini/v1beta/openai/embeddings'].post.requestBody" isObject
jsonpath "$.paths['/ollama/api/chat'].post.requestBody" isObject
jsonpath "$.paths['/ollama/api/embed'].post.requestBody" isObject
jsonpath "$.components.schemas.ChatRequest" isObject
jsonpath "$.components.schemas.MessagesRequest" isObject
jsonpath "$.components.schemas.GenerateContentRequest" isObject
jsonpath "$.components.schemas.ChatRequest2" isObject
jsonpath "$.components.schemas.OpenAiFailureResponse" isObject
jsonpath "$.components.schemas.AnthropicFailureResponse" isObject
jsonpath "$.components.schemas.GeminiFailureResponse" isObject
jsonpath "$.components.schemas.OllamaFailureResponse" isObject
jsonpath "$.components.schemas.OpenAiEmbeddingRequest" isObject
jsonpath "$.components.schemas.OpenAiEmbeddingResponse" isObject
jsonpath "$.components.schemas.ImageGenerationRequest" isObject
jsonpath "$.components.schemas.ImageGenerationResponse" isObject
jsonpath "$.components.schemas.GeminiEmbedContentRequest" isObject
jsonpath "$.components.schemas.GeminiBatchEmbedContentsRequest" isObject
jsonpath "$.components.schemas.OllamaEmbeddingRequest" isObject
jsonpath "$.components.schemas.OllamaEmbeddingResponse" isObject
jsonpath "$.components.schemas.ChatRequest.properties.response_format" isObject
jsonpath "$.components.schemas.ResponsesRequest.properties.text" isObject
jsonpath "$.components.schemas.MessagesRequest.properties.output_config" isObject
jsonpath "$.components.schemas.GenerateConfig.properties.responseFormat" isObject
jsonpath "$.components.schemas.GenerateConfig.properties.responseMimeType" isObject
jsonpath "$.components.schemas.GenerateConfig.properties.responseJsonSchema" isObject
jsonpath "$.components.schemas.ResponseFormat.properties.text" isObject
jsonpath "$.components.schemas.ResponseFormat.properties.image" isObject
jsonpath "$.components.schemas.ChatRequest2.properties.format" isObject
jsonpath "$.components.schemas.ChatRequestResponseFormat" isObject
jsonpath "$.components.schemas.ResponsesRequestTextFormat" isObject
jsonpath "$.components.schemas.OutputConfig" isObject
jsonpath "$.components.schemas.OutputFormat" isObject
jsonpath "$.components.schemas.TextResponseFormat" isObject
jsonpath "$.components.schemas.OutputFormat2" isObject
jsonpath "$.components.schemas.ChatImageUrl" isObject
jsonpath "$.components.schemas.ImageDetail" isObject
jsonpath "$.components.schemas.MessageImageSource" isObject
jsonpath "$.components.schemas.ContentInlineData" isObject
jsonpath "$.components.schemas.ContentFileData" isObject
