POST {{base_url}}/openai/v1/images/generations
Content-Type: application/json
{
  "model": "eliza-retro-image",
  "prompt": "  retro\n cats   dream  ",
  "n": 1,
  "size": "1024x1024",
  "output_format": "png",
  "response_format": "b64_json",
  "user": "fixture-user",
  "future_field": true
}
HTTP 200
[Captures]
openai_png: jsonpath "$.data[0].b64_json"
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 4
jsonpath "$.created" isInteger
jsonpath "$.data" count == 1
jsonpath "$.data[0].*" count == 2
jsonpath "$.data[0].b64_json" startsWith "iVBOR"
jsonpath "$.data[0].revised_prompt" == "retro cats dream"
jsonpath "$.output_format" == "png"
jsonpath "$.size" == "1024x1024"

POST {{base_url}}/gemini/v1beta/models/eliza-retro-image:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "retro cats dream"}]}],
  "generationConfig": {
    "responseModalities": ["IMAGE"],
    "responseFormat": {"image": {"aspectRatio": "1:1", "imageSize": "1K"}}
  }
}
HTTP 200
[Asserts]
jsonpath "$.*" count == 3
jsonpath "$.candidates" count == 1
jsonpath "$.candidates[0].content.role" == "model"
jsonpath "$.candidates[0].content.parts" count == 1
jsonpath "$.candidates[0].content.parts[0].inlineData.mimeType" == "image/png"
jsonpath "$.candidates[0].content.parts[0].inlineData.data" == "{{openai_png}}"
jsonpath "$.candidates[0].finishReason" == "STOP"
jsonpath "$.modelVersion" == "eliza-retro-image"
jsonpath "$.usageMetadata.promptTokenCount" == 3
jsonpath "$.usageMetadata.candidatesTokenCount" == 0
jsonpath "$.usageMetadata.totalTokenCount" == 3
