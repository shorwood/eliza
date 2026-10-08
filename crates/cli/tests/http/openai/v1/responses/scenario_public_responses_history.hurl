# Provider signatures and thinking blocks may be returned in history.
POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-1966",
  "input": [
    {"type": "reasoning", "summary": [{"type": "summary_text", "text": "prior trace"}], "encrypted_content": "hJ1c6IwuPTM="},
    {"type": "message", "role": "user", "content": "Bananas"}
  ]
}
HTTP 200
[Asserts]
jsonpath "$.output" count == 1
jsonpath "$.output[0].type" == "message"
