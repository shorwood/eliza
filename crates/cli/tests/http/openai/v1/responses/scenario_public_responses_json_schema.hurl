POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-1966",
  "input": "I am sad",
  "text": {
    "format": {
      "type": "json_schema",
      "name": "answer",
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
jsonpath "$.output_text" == "{\"answer\":\"I AM SORRY TO HEAR YOU ARE SAD\",\"ok\":false}"
jsonpath "$.output[0].content[0].text" == "{\"answer\":\"I AM SORRY TO HEAR YOU ARE SAD\",\"ok\":false}"
jsonpath "$.usage.input_tokens" > 3
