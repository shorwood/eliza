POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{"model": "eliza-1966", "input": "Hello", "max_output_tokens": 0}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::invalid_max_output_tokens"
