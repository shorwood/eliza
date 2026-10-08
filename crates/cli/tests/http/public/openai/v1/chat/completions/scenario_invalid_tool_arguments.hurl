# A tool directive requires object arguments.
POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "tools": [{"type": "function", "function": {"name": "echo", "parameters": {"type": "object"}}}],
  "messages": [{"role": "user", "content": "@tool echo []"}]
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::turn::invalid_tool_arguments"
