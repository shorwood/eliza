# A required tool choice needs an explicit directive.
POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "tools": [{"type": "function", "function": {"name": "echo", "parameters": {"type": "object"}}}],
  "tool_choice": "required",
  "messages": [{"role": "user", "content": "Hello"}]
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::turn::required_tool_directive"
