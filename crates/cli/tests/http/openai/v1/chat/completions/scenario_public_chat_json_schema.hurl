POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "messages": [{"role": "user", "content": "I am sad"}],
  "response_format": {
    "type": "json_schema",
    "json_schema": {
      "name": "answer",
      "description": "Deterministic fixture",
      "strict": true,
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
jsonpath "$.usage.prompt_tokens" > 3
jsonpath "$.usage.completion_tokens" == 8
