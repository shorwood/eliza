GET {{base_url}}/v1beta/models
HTTP 404
[Asserts]
header "Content-Length" == "0"
body isEmpty
