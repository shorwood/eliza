POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-doctor",
  "input": "I need help"
}
HTTP 200
[Asserts]
jsonpath "$.object" == "response"
jsonpath "$.output[0].content[0].type" == "output_text"
