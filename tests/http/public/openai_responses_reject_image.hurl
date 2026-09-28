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
header "Content-Type" startsWith "application/json"
header "x-eliza-error-code" == "eliza::openai::unsupported_content_part"
jsonpath "$" isObject
jsonpath "$.error.*" count == 4
jsonpath "$.error.message" == "only text content parts are supported"
jsonpath "$.error.type" == "invalid_request_error"
jsonpath "$.error.param" == "input.content"
jsonpath "$.error.code" == "eliza::openai::unsupported_content_part"
