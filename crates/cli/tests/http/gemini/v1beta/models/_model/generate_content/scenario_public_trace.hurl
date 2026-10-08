POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "I am sad"}]}],
  "generationConfig": {
    "maxOutputTokens": 128,
    "thinkingConfig": {"includeThoughts": true, "thinkingLevel": "HIGH"}
  }
}
HTTP 200
[Asserts]
jsonpath "$.candidates[0].content.parts" count == 2
jsonpath "$.candidates[0].content.parts[0].thought" == true
jsonpath "$.candidates[0].content.parts[0].text" == "NORMALIZED INPUT: YOU ARE SAD\nRANKED KEYWORDS: I AM\nSELECTED RULE: I/1/0\nRESPONSE SOURCE: KEYWORD"
jsonpath "$.candidates[0].content.parts[0].thoughtSignature" == "hJ1c6IwuPTM="
jsonpath "$.candidates[0].content.parts[1].text" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.usageMetadata.candidatesTokenCount" == 8
jsonpath "$.usageMetadata.thoughtsTokenCount" == 15
jsonpath "$.usageMetadata.totalTokenCount" == 26
