POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{
  "contents": [{
    "role": "user",
    "parts": [
      {"text": "I am sad"},
      {"inlineData": {"mimeType": "image/png", "data": "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABAQAAAADcWUInAAAACklEQVQI12NwAAAAQgBBg7nsrQAAAABJRU5ErkJggg=="}}
    ]
  }]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.candidates[0].content.parts[0].text" == "[RETRO VISION 1: 2x1 LANDSCAPE, MIDTONE, NEUTRAL, HIGH CONTRAST, 50% FOREGROUND]\nI AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.usageMetadata.promptTokenCount" == 14
jsonpath "$.usageMetadata.candidatesTokenCount" == 19
