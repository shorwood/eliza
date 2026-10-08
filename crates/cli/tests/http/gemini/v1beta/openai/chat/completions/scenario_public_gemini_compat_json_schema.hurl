POST {{base_url}}/gemini/v1beta/openai/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "messages": [{"role": "user", "content": "I am sad"}],
  "response_format": {
    "type": "json_schema",
    "json_schema": {
      "name": "answer",
      "schema": {
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
}
HTTP 200
[Asserts]
jsonpath "$.choices[0].message.content" == "{\"answer\":\"I AM SORRY TO HEAR YOU ARE SAD\",\"ok\":false}"
