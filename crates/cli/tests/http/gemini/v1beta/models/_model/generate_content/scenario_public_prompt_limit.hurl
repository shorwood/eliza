# Verify Gemini reports the same limit through its own response envelope.
POST {{base_url}}/gemini/v1beta/models/eliza-retro-image:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "{{oversized_prompt}}"}]}],
  "generationConfig": {"responseModalities": ["IMAGE"]}
}
HTTP 413
[Asserts]
header "x-eliza-error-code" == "eliza::image::generation::prompt_too_large"
