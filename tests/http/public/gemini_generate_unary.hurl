POST {{base_url}}/gemini/v1beta/models/eliza-doctor:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "I am sad"}]}],
  "futureField": true
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 3
jsonpath "$.candidates" count == 1
jsonpath "$.candidates[0].*" count == 3
jsonpath "$.candidates[0].content.role" == "model"
jsonpath "$.candidates[0].content.parts" count == 1
jsonpath "$.candidates[0].content.parts[0].text" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.candidates[0].finishReason" == "STOP"
jsonpath "$.candidates[0].index" == 0
jsonpath "$.modelVersion" == "eliza-doctor"
jsonpath "$.usageMetadata.promptTokenCount" == 3
jsonpath "$.usageMetadata.candidatesTokenCount" == 8
jsonpath "$.usageMetadata.totalTokenCount" == 11
