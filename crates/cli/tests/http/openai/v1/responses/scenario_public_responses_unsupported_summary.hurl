POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{"model": "eliza-1966", "input": "Hello", "reasoning": {"generate_summary": "verbose"}}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::unsupported_reasoning_summary"
jsonpath "$.error.param" == "reasoning.generate_summary"
