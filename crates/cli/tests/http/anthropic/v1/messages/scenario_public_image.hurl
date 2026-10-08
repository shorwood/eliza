POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{
  "model": "eliza-1966",
  "max_tokens": 128,
  "messages": [{
    "role": "user",
    "content": [
      {"type": "text", "text": "I am sad"},
      {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABAQAAAADcWUInAAAACklEQVQI12NwAAAAQgBBg7nsrQAAAABJRU5ErkJggg=="}}
    ]
  }]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.content[0].text" == "[RETRO VISION 1: 2x1 LANDSCAPE, MIDTONE, NEUTRAL, HIGH CONTRAST, 50% FOREGROUND]\nI AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.usage.input_tokens" == 14
jsonpath "$.usage.output_tokens" == 19
