POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": false,
  "messages": [{
    "role": "user",
    "content": "I am sad",
    "images": ["iVBORw0KGgoAAAANSUhEUgAAAAIAAAABAQAAAADcWUInAAAACklEQVQI12NwAAAAQgBBg7nsrQAAAABJRU5ErkJggg=="]
  }]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.message.content" == "[RETRO VISION 1: 2x1 LANDSCAPE, MIDTONE, NEUTRAL, HIGH CONTRAST, 50% FOREGROUND]\nI AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.prompt_eval_count" == 14
jsonpath "$.eval_count" == 19
