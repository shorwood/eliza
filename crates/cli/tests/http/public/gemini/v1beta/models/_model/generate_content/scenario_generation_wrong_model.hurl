POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "pixels"}]}],
  "generationConfig": {"responseModalities": ["IMAGE"]}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::image_generation_model_required"
jsonpath "$.error.status" == "INVALID_ARGUMENT"
