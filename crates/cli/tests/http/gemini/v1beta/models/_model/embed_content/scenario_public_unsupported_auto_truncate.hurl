POST {{base_url}}/gemini/v1beta/models/fnv-embed:embedContent
Content-Type: application/json
{
  "model": "models/fnv-embed",
  "content": {"parts": [{"text": "Hello"}]},
  "embedContentConfig": {"autoTruncate": true}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::unsupported_embedding_control"
jsonpath "$.error.status" == "INVALID_ARGUMENT"
jsonpath "$.error.message" == "unsupported embedding control `embedContentConfig.autoTruncate`"
