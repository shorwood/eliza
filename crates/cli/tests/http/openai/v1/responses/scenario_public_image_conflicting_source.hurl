POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-1966",
  "input": [{"type": "message", "role": "user", "content": [{"type": "input_image", "image_url": "https://example.invalid/image.png", "file_id": "file_123"}]}]
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::image::conflicting_sources"
jsonpath "$.error.type" == "invalid_request_error"
jsonpath "$.error.param" == "input"
jsonpath "$.error.code" == "eliza::image::conflicting_sources"
