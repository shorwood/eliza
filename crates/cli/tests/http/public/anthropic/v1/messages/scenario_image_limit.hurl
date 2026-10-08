POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{
  "model": "eliza-1966",
  "max_tokens": 128,
  "messages": [{"role": "user", "content": [
    {"type": "image", "source": {"type": "url", "url": "https://example.invalid/1.png"}},
    {"type": "image", "source": {"type": "url", "url": "https://example.invalid/2.png"}},
    {"type": "image", "source": {"type": "url", "url": "https://example.invalid/3.png"}},
    {"type": "image", "source": {"type": "url", "url": "https://example.invalid/4.png"}},
    {"type": "image", "source": {"type": "url", "url": "https://example.invalid/5.png"}}
  ]}]
}
HTTP 413
[Asserts]
header "x-eliza-error-code" == "eliza::image::too_many"
jsonpath "$.type" == "error"
jsonpath "$.error.type" == "request_too_large"
