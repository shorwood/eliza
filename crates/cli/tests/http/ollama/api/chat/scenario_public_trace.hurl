POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": false,
  "think": "high",
  "messages": [{"role": "user", "content": "I am sad"}]
}
HTTP 200
[Asserts]
jsonpath "$.message.content" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.message.thinking" == "NORMALIZED INPUT: YOU ARE SAD\nRANKED KEYWORDS: I AM\nSELECTED RULE: I/1/0\nRESPONSE SOURCE: KEYWORD"
jsonpath "$.eval_count" == 23
