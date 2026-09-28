POST {{base_url}}/gemini/v1beta/models/eliza-doctor:generateContent
Content-Type: application/json
{
  "tools": [{"functionDeclarations": [{
    "name": "echo",
    "description": "Echo a value",
    "parameters": {"type": "object"}
  }]}],
  "contents": [{"role": "user", "parts": [{
    "text": "@tool echo {\"value\":\"hello\"}"
  }]}]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 3
jsonpath "$.candidates" count == 1
jsonpath "$.candidates[0].content.role" == "model"
jsonpath "$.candidates[0].content.parts" count == 1
jsonpath "$.candidates[0].content.parts[0].functionCall.id" startsWith "call_"
jsonpath "$.candidates[0].content.parts[0].functionCall.name" == "echo"
jsonpath "$.candidates[0].content.parts[0].functionCall.args.value" == "hello"
jsonpath "$.candidates[0].finishReason" == "STOP"
jsonpath "$.candidates[0].index" == 0
jsonpath "$.modelVersion" == "eliza-doctor"
jsonpath "$.usageMetadata.promptTokenCount" isInteger
jsonpath "$.usageMetadata.candidatesTokenCount" isInteger
jsonpath "$.usageMetadata.totalTokenCount" isInteger
