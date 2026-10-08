POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{"model": "eliza-1966", "max_tokens": 1024, "thinking": {"type": "enabled", "budget_tokens": 1024}, "messages": [{"role": "user", "content": "Hello"}]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::anthropic::thinking_budget_too_large"
