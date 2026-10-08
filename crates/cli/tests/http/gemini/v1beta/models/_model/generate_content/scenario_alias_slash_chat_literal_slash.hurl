# A literal slash in the advertised chat name must route.
POST {{base_url}}/gemini/v1beta/models/acme/chat:generateContent
Content-Type: application/json
{"contents":[{"role":"user","parts":[{"text":"Hello"}]}]}
HTTP 200
[Asserts]
jsonpath "$.modelVersion" == "acme/chat"
