# Reject an oversized upload outside provider routes as a linked RFC problem.
POST {{base_url}}/not-a-provider
Content-Type: application/json
{"oversized":"{{oversized_request}}"}
HTTP 413
[Asserts]
header "Content-Type" startsWith "application/problem+json"
header "Link" == "<https://eliza.hsjm.io/docs/errors#request-too-large>; rel=\"describedby\""
jsonpath "$.type" == "https://eliza.hsjm.io/docs/errors#request-too-large"
jsonpath "$.status" == 413
