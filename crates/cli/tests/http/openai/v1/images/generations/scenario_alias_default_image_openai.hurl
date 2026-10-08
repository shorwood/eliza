# Image aliases include omitted-model defaults and Gemini's inferred mode.
POST {{base_url}}/openai/v1/images/generations
Authorization: Bearer {{token}}
Content-Type: application/json
{"prompt":"retro cats","size":"256x256"}
HTTP 200
[Asserts]
jsonpath "$.data[0].b64_json" startsWith "iVBOR"
