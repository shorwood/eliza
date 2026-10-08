POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{
  "model": "eliza-1966",
  "max_tokens": 128,
  "messages": [{"role": "user", "content": "I am sad"}],
  "output_config": {
    "format": {
      "type": "json_schema",
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
jsonpath "$.content[0].text" == "{\"answer\":\"I AM SORRY TO HEAR YOU ARE SAD\",\"ok\":false}"
jsonpath "$.usage.input_tokens" > 3
