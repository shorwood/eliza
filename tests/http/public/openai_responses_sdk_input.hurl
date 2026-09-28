POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-doctor",
  "input": [{
    "type": "message",
    "role": "user",
    "content": [{"type": "input_text", "text": "I am sad"}]
  }]
}
HTTP 200
[Asserts]
jsonpath "$.output[0].content[0].text" == "I AM SORRY TO HEAR YOU ARE SAD"
