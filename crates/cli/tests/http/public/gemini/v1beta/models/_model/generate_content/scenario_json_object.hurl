POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "I am sad"}]}],
  "generationConfig": {
    "responseMimeType": "application/json",
    "responseJsonSchema": {
      "type": "object",
      "properties": {
        "answer": {"type": "string"},
        "ok": {"type": "boolean"}
      },
      "required": ["answer", "ok"],
      "additionalProperties": false
    }
  }
}
HTTP 200
[Asserts]
jsonpath "$.candidates[0].content.parts[0].text" == "{\"answer\":\"I AM SORRY TO HEAR YOU ARE SAD\",\"ok\":false}"
