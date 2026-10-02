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

POST {{base_url}}/openai/v1/images/generations
Content-Type: application/json
{"prompt": "ordered variations", "n": 2, "size": "256x256"}
HTTP 200
[Captures]
variation_zero: jsonpath "$.data[0].b64_json"
variation_one: jsonpath "$.data[1].b64_json"
[Asserts]
jsonpath "$.data" count == 2
variable "variation_zero" != "{{variation_one}}"

POST {{base_url}}/gemini/v1beta/models/eliza-retro-image:generateContent
Content-Type: application/json
{"contents": [{"role": "user", "parts": [{"text": "I am sad"}]}]}
HTTP 200
[Asserts]
jsonpath "$.candidates[0].content.parts" count == 2
jsonpath "$.candidates[0].content.parts[0].text" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.candidates[0].content.parts[1].inlineData.mimeType" == "image/png"
jsonpath "$.candidates[0].content.parts[1].inlineData.data" startsWith "iVBOR"
jsonpath "$.usageMetadata.promptTokenCount" == 3
jsonpath "$.usageMetadata.candidatesTokenCount" == 8
jsonpath "$.usageMetadata.totalTokenCount" == 11

POST {{base_url}}/gemini/v1beta/models/eliza-retro-image:streamGenerateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "json stream pixels"}]}],
  "generationConfig": {"responseModalities": ["IMAGE"]}
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$" count == 1
jsonpath "$[0].candidates[0].content.parts[0].inlineData.mimeType" == "image/png"
jsonpath "$[0].candidates[0].finishReason" == "STOP"

POST {{base_url}}/gemini/v1beta/models/eliza-retro-image:streamGenerateContent?alt=sse
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "sse pixels"}]}],
  "generationConfig": {"responseModalities": ["IMAGE", "TEXT"]}
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "data: {\"candidates\""
body contains "\"text\":"
body contains "\"inlineData\":{\"mimeType\":\"image/png\",\"data\":\"iVBOR"
body contains "\"finishReason\":\"STOP\""
body contains "\"modelVersion\":\"eliza-retro-image\""
