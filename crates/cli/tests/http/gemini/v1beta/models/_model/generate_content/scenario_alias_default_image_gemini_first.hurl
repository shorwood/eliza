POST {{base_url}}/gemini/v1beta/models/image-one:generateContent
x-goog-api-key: {{token}}
Content-Type: application/json
{"contents":[{"role":"user","parts":[{"text":"retro cats"}]}]}
HTTP 200
[Asserts]
jsonpath "$.modelVersion" == "image-one"
jsonpath "$.candidates[0].content.parts[1].inlineData.mimeType" == "image/png"
