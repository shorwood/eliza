# Keep a question attached when a client sends its image as the next user message.
POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-1966",
  "input": [
    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "What can you measure in this image?"}]},
    {"type": "message", "role": "user", "content": [{"type": "input_image", "image_url": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABAQAAAADcWUInAAAACklEQVQI12NwAAAAQgBBg7nsrQAAAABJRU5ErkJggg=="}]}
  ]
}
HTTP 200
[Asserts]
jsonpath "$.output_text" == "[RETRO VISION 1: 2x1 LANDSCAPE, MIDTONE, NEUTRAL, HIGH CONTRAST, 50% FOREGROUND]\nWHY DO YOU ASK"
