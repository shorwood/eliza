POST {{base_url}}/gemini/v1beta/models/eliza-doctor:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "I am sad"}]}],
  "futureField": true
}
HTTP 200
[Asserts]
jsonpath "$.candidates[0].content.role" == "model"
jsonpath "$.candidates[0].content.parts[0].text" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.candidates[0].finishReason" == "STOP"
