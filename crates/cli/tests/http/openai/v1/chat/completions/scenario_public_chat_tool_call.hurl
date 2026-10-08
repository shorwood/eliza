POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "tools": [{
    "type": "function",
    "function": {
      "name": "echo",
      "description": "Echo a value",
      "parameters": {
        "type": "object",
        "properties": {"value": {"type": "string"}},
        "required": ["value"]
      }
    }
  }],
  "messages": [{"role": "user", "content": "@tool echo {\"value\":\"hello\"}"}]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 6
jsonpath "$.id" startsWith "chatcmpl-"
jsonpath "$.object" == "chat.completion"
jsonpath "$.model" == "eliza-1966"
jsonpath "$.choices" count == 1
jsonpath "$.choices[0].index" == 0
jsonpath "$.choices[0].finish_reason" == "tool_calls"
jsonpath "$.choices[0].message.role" == "assistant"
jsonpath "$.choices[0].message.content" == null
jsonpath "$.choices[0].message.tool_calls" count == 1
jsonpath "$.choices[0].message.tool_calls[0].id" startsWith "call_"
jsonpath "$.choices[0].message.tool_calls[0].type" == "function"
jsonpath "$.choices[0].message.tool_calls[0].function.name" == "echo"
jsonpath "$.choices[0].message.tool_calls[0].function.arguments" == "{\"value\":\"hello\"}"
jsonpath "$.usage.prompt_tokens" isInteger
jsonpath "$.usage.completion_tokens" isInteger
jsonpath "$.usage.total_tokens" isInteger
