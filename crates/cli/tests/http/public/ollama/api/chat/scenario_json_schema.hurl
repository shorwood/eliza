POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": false,
  "messages": [{"role": "user", "content": "I am sad"}],
  "format": {
    "type": "object",
    "properties": {
      "answer": {"type": "string"},
      "ok": {"type": "boolean"}
    },
    "required": ["answer", "ok"],
    "additionalProperties": false
  }
}
HTTP 200
[Asserts]
jsonpath "$.message.content" == "{\"answer\":\"I AM SORRY TO HEAR YOU ARE SAD\",\"ok\":false}"
jsonpath "$.prompt_eval_count" > 3
