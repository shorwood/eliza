POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "I am sad"}]}],
  "generationConfig": {"responseMimeType": "text/plain"}
}
HTTP 200
[Asserts]
jsonpath "$.candidates[0].content.parts[0].text" == "I AM SORRY TO HEAR YOU ARE SAD"
