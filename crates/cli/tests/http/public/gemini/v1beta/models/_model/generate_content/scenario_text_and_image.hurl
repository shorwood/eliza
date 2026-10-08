POST {{base_url}}/gemini/v1beta/models/eliza-retro-image:generateContent
Content-Type: application/json
{"contents": [{"role": "user", "parts": [{"text": "I am sad"}]}]}
HTTP 200
[Asserts]
jsonpath "$.candidates[0].content.parts" count == 2
jsonpath "$.candidates[0].content.parts[0].text" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.candidates[0].content.parts[1].inlineData.mimeType" == "image/png"
jsonpath "$.candidates[0].content.parts[1].inlineData.data" startsWith "iVBOR"
jsonpath "$.usageMetadata.promptTokenCount" == 3
jsonpath "$.usageMetadata.candidatesTokenCount" == 8
jsonpath "$.usageMetadata.totalTokenCount" == 11
