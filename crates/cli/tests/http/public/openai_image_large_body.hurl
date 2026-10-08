# Verify that a JSON body above Axum's former two-mebibyte limit reaches image validation.
POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "messages": [{"role": "user", "content": [{"type": "image_url", "image_url": {"url": "data:image/png;base64,{{oversized_image}}"}}]}]
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::image::malformed"
