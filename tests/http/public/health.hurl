GET {{base_url}}/healthz
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 2
jsonpath "$.status" == "ok"
jsonpath "$.service" == "eliza"
