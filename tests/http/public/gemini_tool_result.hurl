POST {{base_url}}/gemini/v1beta/models/eliza-doctor:generateContent
Content-Type: application/json
{
  "tools": [{"functionDeclarations": [{
    "name": "echo",
    "parameters": {"type": "object"}
  }]}],
  "contents": [
    {"role": "user", "parts": [{"text": "@tool echo {\"value\":\"hello\"}"}]},
    {"role": "model", "parts": [{
      "functionCall": {"name": "echo", "args": {"value": "hello"}}
    }]},
    {"role": "user", "parts": [{
      "functionResponse": {"name": "echo", "response": {"value": "hello"}}
    }]}
  ]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 3
jsonpath "$.candidates" count == 1
jsonpath "$.candidates[0].content.role" == "model"
jsonpath "$.candidates[0].content.parts" count == 1
jsonpath "$.candidates[0].content.parts[0].text" == "TOOL CALL COMPLETE"
jsonpath "$.candidates[0].finishReason" == "STOP"
jsonpath "$.candidates[0].index" == 0
jsonpath "$.modelVersion" == "eliza-doctor"
jsonpath "$.usageMetadata.promptTokenCount" isInteger
jsonpath "$.usageMetadata.candidatesTokenCount" isInteger
jsonpath "$.usageMetadata.totalTokenCount" isInteger
