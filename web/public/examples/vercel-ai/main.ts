import assert from 'node:assert/strict';
import { createOpenAI } from '@ai-sdk/openai';
import { generateText, streamText } from 'ai';

const root = process.env.ELIZA_API_ROOT;
if (!root) throw new Error('Set ELIZA_API_ROOT to an existing instance');
const openai = createOpenAI({
  apiKey: process.env.ELIZA_API_KEY ?? 'local',
  baseURL: `${root.replace(/\/$/, '')}/openai/v1`,
});
const request = {
  model: openai.chat('eliza-1966'),
  prompt: 'I am sad.',
  maxOutputTokens: 128,
  maxRetries: 0,
  abortSignal: AbortSignal.timeout(30_000),
};
const completion = await generateText(request);
assert.ok(completion.text.length > 0);
let streamed = '';
for await (const part of streamText(request).fullStream) {
  if (part.type === 'text-delta') streamed += part.text;
  if (part.type === 'error') throw part.error;
}
assert.equal(streamed, completion.text);
console.log(completion.text);
