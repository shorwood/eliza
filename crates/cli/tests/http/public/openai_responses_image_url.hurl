POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-1966",
  "input": [{
    "type": "message",
    "role": "user",
    "content": [{"type": "input_image", "image_url": "http://127.0.0.1:1/unreachable.png"}]
  }]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.output[0].content[0].text" == "[RETRO VISION 1: OPAQUE IMAGE NOT INSPECTED]\nI AM NOT SURE I UNDERSTAND YOU FULLY"
jsonpath "$.output_text" == "[RETRO VISION 1: OPAQUE IMAGE NOT INSPECTED]\nI AM NOT SURE I UNDERSTAND YOU FULLY"
jsonpath "$.usage.input_tokens" == 7
