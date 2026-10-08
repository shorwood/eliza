POST {{base_url}}/openai/v1/images/generations
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"image-two","prompt":"retro cats","size":"256x256"}
HTTP 200
[Asserts]
jsonpath "$.data[0].b64_json" startsWith "iVBOR"
