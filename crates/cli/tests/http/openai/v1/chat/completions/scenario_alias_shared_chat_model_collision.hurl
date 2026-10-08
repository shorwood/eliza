POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{"model":"fnv-embed","messages":[{"role":"user","content":"I need help"}]}
HTTP 200
[Asserts]
jsonpath "$.model" == "fnv-embed"
jsonpath "$.choices[0].message.content" != ""
