GET {{base_url}}/v1/messages
HTTP 404
[Asserts]
header "Content-Length" == "0"
body isEmpty
