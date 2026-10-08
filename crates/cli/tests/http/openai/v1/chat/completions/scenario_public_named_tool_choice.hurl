# A named choice excludes calls to other offered functions.
POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "tools": [
    {"type": "function", "function": {"name": "echo", "parameters": {"type": "object"}}},
    {"type": "function", "function": {"name": "other", "parameters": {"type": "object"}}}
  ],
  "tool_choice": {"type": "function", "function": {"name": "other"}},
  "messages": [{"role": "user", "content": "@tool echo {}"}]
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::turn::tool_choice_disallows"
