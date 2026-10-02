POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "messages": [{
    "role": "user",
    "content": [
      {"type": "text", "text": "I am sad"},
      {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABAQAAAADcWUInAAAACklEQVQI12NwAAAAQgBBg7nsrQAAAABJRU5ErkJggg==", "detail": "low"}}
    ]
  }]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.choices[0].message.content" == "[RETRO VISION 1: 2x1 LANDSCAPE, MIDTONE, NEUTRAL, HIGH CONTRAST, 50% FOREGROUND]\nI AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.usage.prompt_tokens" == 14
jsonpath "$.usage.completion_tokens" == 19
