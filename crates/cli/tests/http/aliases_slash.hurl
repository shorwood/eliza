# The catalog advertises literal model resource names.
GET {{base_url}}/gemini/v1beta/models
HTTP 200
[Asserts]
jsonpath "$.models[0].name" == "models/acme/chat"
jsonpath "$.models[2].name" == "models/acme/embed"

# A literal slash in the advertised chat name must route.
POST {{base_url}}/gemini/v1beta/models/acme/chat:generateContent
Content-Type: application/json
{"contents":[{"role":"user","parts":[{"text":"Hello"}]}]}
HTTP 200
[Asserts]
jsonpath "$.modelVersion" == "acme/chat"

# The existing percent-encoded chat path stays valid.
POST {{base_url}}/gemini/v1beta/models/acme%2Fchat:generateContent
Content-Type: application/json
{"contents":[{"role":"user","parts":[{"text":"Hello"}]}]}
HTTP 200
[Asserts]
jsonpath "$.modelVersion" == "acme/chat"

# A literal slash in the embedding name must route.
POST {{base_url}}/gemini/v1beta/models/acme/embed:embedContent
Content-Type: application/json
{"model":"models/acme/embed","content":{"parts":[{"text":"Hello"}]},"outputDimensionality":8}
HTTP 200
[Asserts]
jsonpath "$.embedding.values" count == 8

# The existing percent-encoded embedding path stays valid.
POST {{base_url}}/gemini/v1beta/models/acme%2Fembed:embedContent
Content-Type: application/json
{"model":"models/acme/embed","content":{"parts":[{"text":"Hello"}]},"outputDimensionality":8}
HTTP 200
[Asserts]
jsonpath "$.embedding.values" count == 8

# Batch embeddings use the same literal model resource name.
POST {{base_url}}/gemini/v1beta/models/acme/embed:batchEmbedContents
Content-Type: application/json
{"requests":[{"model":"models/acme/embed","content":{"parts":[{"text":"Hello"}]},"outputDimensionality":8}]}
HTTP 200
[Asserts]
jsonpath "$.embeddings" count == 1
jsonpath "$.embeddings[0].values" count == 8
