POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-1966",
  "input": "I am sad",
  "reasoning": {"effort": "high", "summary": "auto"},
  "max_output_tokens": 128
}
HTTP 200
[Asserts]
jsonpath "$.output" count == 2
jsonpath "$.output[0].type" == "reasoning"
jsonpath "$.output[0].summary[0].text" == "NORMALIZED INPUT: YOU ARE SAD\nRANKED KEYWORDS: I AM\nSELECTED RULE: I/1/0\nRESPONSE SOURCE: KEYWORD"
jsonpath "$.output[0].encrypted_content" == "hJ1c6IwuPTM="
jsonpath "$.output[1].type" == "message"
jsonpath "$.output_text" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.usage.output_tokens" == 23
jsonpath "$.usage.output_tokens_details.reasoning_tokens" == 15
jsonpath "$.usage.total_tokens" == 26

POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{
  "model": "eliza-1966",
  "max_tokens": 2048,
  "thinking": {"type": "enabled", "budget_tokens": 1024},
  "output_config": {"effort": "high"},
  "messages": [{"role": "user", "content": "I am sad"}]
}
HTTP 200
[Asserts]
jsonpath "$.content" count == 2
jsonpath "$.content[0].type" == "thinking"
jsonpath "$.content[0].thinking" == "NORMALIZED INPUT: YOU ARE SAD\nRANKED KEYWORDS: I AM\nSELECTED RULE: I/1/0\nRESPONSE SOURCE: KEYWORD"
jsonpath "$.content[0].signature" == "hJ1c6IwuPTM="
jsonpath "$.content[1].text" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.usage.output_tokens" == 23

POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{
  "model": "eliza-1966",
  "max_tokens": 2048,
  "thinking": {"type": "adaptive", "display": "omitted"},
  "messages": [{"role": "user", "content": "I am sad"}]
}
HTTP 200
[Asserts]
jsonpath "$.content" count == 1
jsonpath "$.content[0].type" == "text"
jsonpath "$.usage.output_tokens" == 8

POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "I am sad"}]}],
  "generationConfig": {
    "maxOutputTokens": 128,
    "thinkingConfig": {"includeThoughts": true, "thinkingLevel": "HIGH"}
  }
}
HTTP 200
[Asserts]
jsonpath "$.candidates[0].content.parts" count == 2
jsonpath "$.candidates[0].content.parts[0].thought" == true
jsonpath "$.candidates[0].content.parts[0].text" == "NORMALIZED INPUT: YOU ARE SAD\nRANKED KEYWORDS: I AM\nSELECTED RULE: I/1/0\nRESPONSE SOURCE: KEYWORD"
jsonpath "$.candidates[0].content.parts[0].thoughtSignature" == "hJ1c6IwuPTM="
jsonpath "$.candidates[0].content.parts[1].text" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.usageMetadata.candidatesTokenCount" == 8
jsonpath "$.usageMetadata.thoughtsTokenCount" == 15
jsonpath "$.usageMetadata.totalTokenCount" == 26

POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": false,
  "think": "high",
  "messages": [{"role": "user", "content": "I am sad"}]
}
HTTP 200
[Asserts]
jsonpath "$.message.content" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.message.thinking" == "NORMALIZED INPUT: YOU ARE SAD\nRANKED KEYWORDS: I AM\nSELECTED RULE: I/1/0\nRESPONSE SOURCE: KEYWORD"
jsonpath "$.eval_count" == 23

# Chat surfaces accept effort without adding a nonstandard output extension.
POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "reasoning_effort": "high",
  "messages": [{"role": "user", "content": "I am sad"}]
}
HTTP 200
[Asserts]
jsonpath "$.choices[0].message.content" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.choices[0].message.reasoning_content" not exists
jsonpath "$.usage.total_tokens" == 11

POST {{base_url}}/gemini/v1beta/openai/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "reasoning_effort": "high",
  "messages": [{"role": "user", "content": "I am sad"}]
}
HTTP 200
[Asserts]
jsonpath "$.choices[0].message.content" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.choices[0].message.reasoning_content" not exists
jsonpath "$.usage.total_tokens" == 11

# Provider signatures and thinking blocks may be returned in history.
POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-1966",
  "input": [
    {"type": "reasoning", "summary": [{"type": "summary_text", "text": "prior trace"}], "encrypted_content": "hJ1c6IwuPTM="},
    {"type": "message", "role": "user", "content": "Bananas"}
  ]
}
HTTP 200
[Asserts]
jsonpath "$.output" count == 1
jsonpath "$.output[0].type" == "message"

POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{
  "model": "eliza-1966",
  "max_tokens": 128,
  "messages": [
    {"role": "assistant", "content": [{"type": "thinking", "thinking": "prior trace", "signature": "hJ1c6IwuPTM="}]},
    {"role": "user", "content": "Bananas"}
  ]
}
HTTP 200

POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{
  "contents": [
    {"role": "model", "parts": [{"text": "prior trace", "thought": true, "thoughtSignature": "hJ1c6IwuPTM="}]},
    {"role": "user", "parts": [{"text": "Bananas"}]}
  ]
}
HTTP 200

POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": false,
  "messages": [
    {"role": "assistant", "content": "", "thinking": "prior trace"},
    {"role": "user", "content": "Bananas"}
  ]
}
HTTP 200

# Generic validation rejects invalid controls without model-specific ranges.
POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{"model": "eliza-1966", "reasoning_effort": "extreme", "messages": [{"role": "user", "content": "Hello"}]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::unsupported_reasoning_effort"
jsonpath "$.error.param" == "reasoning_effort"

POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{"model": "eliza-1966", "input": "Hello", "reasoning": {"generate_summary": "verbose"}}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::unsupported_reasoning_summary"
jsonpath "$.error.param" == "reasoning.generate_summary"

POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{"model": "eliza-1966", "input": "Hello", "max_output_tokens": 0}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::invalid_max_output_tokens"

POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{"model": "eliza-1966", "max_tokens": 1024, "thinking": {"type": "enabled", "budget_tokens": 1024}, "messages": [{"role": "user", "content": "Hello"}]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::anthropic::thinking_budget_too_large"

POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{"contents": [{"parts": [{"text": "Hello"}]}], "generationConfig": {"thinkingConfig": {"includeThoughts": true, "thinkingBudget": -2}}}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::invalid_thinking_budget"

POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{"model": "eliza-1966", "think": "", "messages": [{"role": "user", "content": "Hello"}]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::ollama::invalid_think"
