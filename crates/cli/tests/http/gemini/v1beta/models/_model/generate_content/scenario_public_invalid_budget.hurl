POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{"contents": [{"parts": [{"text": "Hello"}]}], "generationConfig": {"thinkingConfig": {"includeThoughts": true, "thinkingBudget": -2}}}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::invalid_thinking_budget"
