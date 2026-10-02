POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "messages": [{"role": "user", "content": [{"type": "image_url", "image_url": {"url": "data:image/png;base64,%%%"}}]}]
}
HTTP 400
[Asserts]
header "Content-Type" startsWith "application/json"
header "x-eliza-error-code" == "eliza::image::invalid_base64"
jsonpath "$.error.type" == "invalid_request_error"
jsonpath "$.error.param" == "messages"
jsonpath "$.error.code" == "eliza::image::invalid_base64"
