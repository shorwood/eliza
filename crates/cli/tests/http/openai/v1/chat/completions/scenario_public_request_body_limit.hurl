# Reject an oversized upload before JSON or provider validation.
POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{"oversized":"{{oversized_request}}"}
HTTP 413
[Asserts]
header "Content-Type" startsWith "application/json"
header "x-eliza-error-code" == "eliza::admission::request_too_large"
header "Link" == "<https://eliza.hsjm.io/docs/errors#request-too-large>; rel=\"describedby\""
jsonpath "$.error.message" == "Request exceeds size limits."
jsonpath "$.error.type" == "invalid_request_error"
