# First request

Set `ELIZA_API_ROOT` to the base of an existing instance, without a trailing slash.
A directly served local instance uses `http://127.0.0.1:8787`. The planned hosted
root is `https://example.com/api`; replace the placeholder after deployment.

```sh
export ELIZA_API_ROOT=http://127.0.0.1:8787
curl --fail-with-body --silent --show-error --connect-timeout 5 --max-time 15 \
  "$ELIZA_API_ROOT/openai/v1/chat/completions" \
  -H 'Content-Type: application/json' \
  -d '{"model":"eliza-1966","messages":[{"role":"user","content":"I am sad."}]}'
```

The response uses OpenAI's Chat Completions envelope, including
`choices[0].message.content`. Request streaming with `"stream":true` and use curl
`--no-buffer`; the transport is SSE and ends with `data: [DONE]`.

Use [the smoke script](sdks.md) to assert a nonempty result, collect a complete
stream, and check a native validation error. [Authentication](authentication.md)
explains headers for protected instances. No real provider credentials are needed.

Each request starts a fresh ELIZA conversation and replays the supplied user
history. The service does not retain a chat session between calls.
