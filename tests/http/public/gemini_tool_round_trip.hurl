POST {{base_url}}/gemini/v1beta/models/eliza-doctor:generateContent
Content-Type: application/json
{
  "tools": [{"functionDeclarations": [{
    "name": "echo",
    "description": "Echo a value",
    "parameters": {"type": "object"}
  }]}],
  "contents": [{"role": "user", "parts": [{
    "text": "@tool echo {\"value\":\"hello\"}"
  }]}]
}
HTTP 200
[Asserts]
jsonpath "$.candidates[0].content.parts[0].functionCall.name" == "echo"
jsonpath "$.candidates[0].content.parts[0].functionCall.args.value" == "hello"

POST {{base_url}}/gemini/v1beta/models/eliza-doctor:generateContent
Content-Type: application/json
{
  "tools": [{"functionDeclarations": [{
    "name": "echo",
    "parameters": {"type": "object"}
  }]}],
  "contents": [
    {"role": "user", "parts": [{"text": "@tool echo {\"value\":\"hello\"}"}]},
    {"role": "model", "parts": [{
      "functionCall": {"name": "echo", "args": {"value": "hello"}}
    }]},
    {"role": "user", "parts": [{
      "functionResponse": {"name": "echo", "response": {"value": "hello"}}
    }]}
  ]
}
HTTP 200
[Asserts]
jsonpath "$.candidates[0].content.parts[0].text" == "TOOL CALL COMPLETE"
