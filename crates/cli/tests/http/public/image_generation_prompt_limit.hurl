# Verify OpenAI rejects a prompt above the image generation limit.
POST {{base_url}}/openai/v1/images/generations
Content-Type: application/json
{"model": "eliza-retro-image", "prompt": "{{oversized_prompt}}"}
HTTP 413
[Asserts]
header "x-eliza-error-code" == "eliza::image::generation::prompt_too_large"

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
