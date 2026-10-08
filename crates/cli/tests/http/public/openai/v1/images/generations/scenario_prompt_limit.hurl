# Verify OpenAI rejects a prompt above the image generation limit.
POST {{base_url}}/openai/v1/images/generations
Content-Type: application/json
{"model": "eliza-retro-image", "prompt": "{{oversized_prompt}}"}
HTTP 413
[Asserts]
header "x-eliza-error-code" == "eliza::image::generation::prompt_too_large"
