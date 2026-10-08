# The existing percent-encoded chat path stays valid.
POST {{base_url}}/gemini/v1beta/models/acme%2Fchat:generateContent
Content-Type: application/json
{"contents":[{"role":"user","parts":[{"text":"Hello"}]}]}
HTTP 200
[Asserts]
jsonpath "$.modelVersion" == "acme/chat"
