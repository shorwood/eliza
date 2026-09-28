POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-doctor",
  "input": [{
    "type": "message",
    "role": "user",
    "content": [{"type": "input_image", "image_url": "https://example.com/image.png"}]
  }]
}
HTTP 400
[Asserts]
jsonpath "$.error.type" == "unsupported_request_error"
jsonpath "$.error.param" == "input.content"
