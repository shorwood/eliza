import { generateText, jsonSchema, stepCountIs, streamText, tool } from 'ai';
import type { LanguageModel } from 'ai';

import { anthropic, CHAT_MODEL, geminiOpenAI, google, openai } from './providers.ts';
import type { Example } from './runner.ts';

const MAX_OUTPUT_TOKENS = 128;
const PROMPT = 'I am sad.';

const CHAT_MODELS = [
  ['OpenAI Chat Completions', openai.chat(CHAT_MODEL)],
  ['OpenAI Responses', openai.responses(CHAT_MODEL)],
  ['Gemini OpenAI-compatible alias', geminiOpenAI(CHAT_MODEL)],
  ['Anthropic Messages', anthropic.messages(CHAT_MODEL)],
  ['Gemini generateContent', google.chat(CHAT_MODEL)],
] satisfies ReadonlyArray<readonly [string, LanguageModel]>;

const TOOL_MODELS = [
  ['OpenAI Chat', openai.chat(CHAT_MODEL)],
  ['OpenAI Responses', openai.responses(CHAT_MODEL)],
  ['Anthropic', anthropic.messages(CHAT_MODEL)],
  ['Gemini', google.chat(CHAT_MODEL)],
] satisfies ReadonlyArray<readonly [string, LanguageModel]>;

const echo = tool({
  description: 'Echo a value',
  inputSchema: jsonSchema<{ value: string }>({
    type: 'object',
    properties: { value: { type: 'string' } },
    required: ['value'],
    additionalProperties: false,
  }),
  execute: async ({ value }) => value,
});

async function complete(model: LanguageModel): Promise<string> {
  return (await generateText({ model, prompt: PROMPT, maxOutputTokens: MAX_OUTPUT_TOKENS })).text;
}

async function stream(model: LanguageModel): Promise<string> {
  const result = streamText({ model, prompt: PROMPT, maxOutputTokens: MAX_OUTPUT_TOKENS });
  let text = '';
  for await (const part of result.stream) {
    if (part.type === 'text-delta') {
      text += part.text;
    } else if (part.type === 'error') {
      throw part.error;
    }
  }
  return text;
}

async function useTool(model: LanguageModel): Promise<string> {
  const result = await generateText({
    model,
    prompt: '@tool echo {"value":"hello"}',
    maxOutputTokens: MAX_OUTPUT_TOKENS,
    tools: { echo },
    stopWhen: stepCountIs(2),
  });
  return result.text;
}

export const chatExamples: Example[] = [
  ...CHAT_MODELS.flatMap(([name, model]) => [
    { name, run: () => complete(model) },
    { name: `${name} stream`, run: () => stream(model) },
  ]),
  ...TOOL_MODELS.map(([name, model]) => ({
    name: `${name} tool round trip`,
    run: () => useTool(model),
  })),
];
