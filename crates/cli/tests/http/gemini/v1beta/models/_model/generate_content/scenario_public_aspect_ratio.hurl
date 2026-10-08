POST {{base_url}}/gemini/v1beta/models/eliza-retro-image:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "pixels"}]}],
  "generationConfig": {
    "responseModalities": ["IMAGE"],
    "responseFormat": {"image": {"aspectRatio": "16:9", "imageSize": "1K"}}
  }
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::unsupported_image_aspect_ratio"
