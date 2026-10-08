POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-1966",
  "input": "I am sad",
  "reasoning": {"effort": "high", "summary": "auto"},
  "max_output_tokens": 128
}
HTTP 200
[Asserts]
jsonpath "$.output" count == 2
jsonpath "$.output[0].type" == "reasoning"
jsonpath "$.output[0].summary[0].text" == "NORMALIZED INPUT: YOU ARE SAD\nRANKED KEYWORDS: I AM\nSELECTED RULE: I/1/0\nRESPONSE SOURCE: KEYWORD"
jsonpath "$.output[0].encrypted_content" == "hJ1c6IwuPTM="
jsonpath "$.output[1].type" == "message"
jsonpath "$.output_text" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.usage.output_tokens" == 23
jsonpath "$.usage.output_tokens_details.reasoning_tokens" == 15
jsonpath "$.usage.total_tokens" == 26
