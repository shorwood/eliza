import { createAnthropic } from '@ai-sdk/anthropic';
import { createGoogle } from '@ai-sdk/google';
import { createOpenAI } from '@ai-sdk/openai';
import { createOpenAICompatible } from '@ai-sdk/openai-compatible';
import { generateText, jsonSchema, stepCountIs, streamText, tool } from 'ai';
import type { LanguageModel } from 'ai';

const BASE_URL = process.env.ELIZA_BASE_URL ?? 'http://127.0.0.1:8787';
const MODEL_ID = 'eliza-doctor';
const MAX_OUTPUT_TOKEN = 128;
const PROMPT = 'I am sad.';

const openai = createOpenAI({
  apiKey: 'local',
  baseURL: `${BASE_URL}/openai/v1`,
});
const geminiOpenAI = createOpenAICompatible({
  apiKey: 'local',
  baseURL: `${BASE_URL}/gemini/v1beta/openai`,
  name: 'eliza-gemini-openai',
});
const anthropic = createAnthropic({
  apiKey: 'local',
  baseURL: `${BASE_URL}/anthropic/v1`,
});
const google = createGoogle({
  apiKey: 'local',
  baseURL: `${BASE_URL}/gemini/v1beta`,
});

const echoTool = tool({
  description: 'Echo a value',
  inputSchema: jsonSchema<{ value: string }>({
    type: 'object',
    properties: { value: { type: 'string' } },
    required: ['value'],
    additionalProperties: false,
  }),
  execute: async ({ value }) => value,
});

type Example = {
  name: string;
  run: () => Promise<string>;
};

async function collectText(stream: AsyncIterable<string>): Promise<string> {
  let text = '';
  for await (const chunk of stream) {
    text += chunk;
  }
  return text;
}

async function toolRoundTrip(model: LanguageModel): Promise<string> {
  return (
    await generateText({
      maxOutputTokens: MAX_OUTPUT_TOKEN,
      model,
      prompt: '@tool echo {"value":"hello"}',
      tools: { echo: echoTool },
      stopWhen: stepCountIs(2),
    })
  ).text;
}

const examples: Example[] = [
  {
    name: 'OpenAI Chat Completions',
    run: async () =>
      (await generateText({ maxOutputTokens: MAX_OUTPUT_TOKEN, model: openai.chat(MODEL_ID), prompt: PROMPT })).text,
  },
  {
    name: 'OpenAI Chat Completions stream',
    run: () =>
      collectText(streamText({ maxOutputTokens: MAX_OUTPUT_TOKEN, model: openai.chat(MODEL_ID), prompt: PROMPT }).textStream),
  },
  {
    name: 'OpenAI Responses',
    run: async () =>
      (await generateText({ maxOutputTokens: MAX_OUTPUT_TOKEN, model: openai.responses(MODEL_ID), prompt: PROMPT })).text,
  },
  {
    name: 'OpenAI Chat tool round trip',
    run: () => toolRoundTrip(openai.chat(MODEL_ID)),
  },
  {
    name: 'OpenAI Responses tool round trip',
    run: () => toolRoundTrip(openai.responses(MODEL_ID)),
  },
  {
    name: 'Gemini OpenAI-compatible alias',
    run: async () =>
      (await generateText({ maxOutputTokens: MAX_OUTPUT_TOKEN, model: geminiOpenAI(MODEL_ID), prompt: PROMPT })).text,
  },
  {
    name: 'Gemini OpenAI-compatible alias stream',
    run: () =>
      collectText(streamText({ maxOutputTokens: MAX_OUTPUT_TOKEN, model: geminiOpenAI(MODEL_ID), prompt: PROMPT }).textStream),
  },
  {
    name: 'Anthropic Messages',
    run: async () =>
      (await generateText({ maxOutputTokens: MAX_OUTPUT_TOKEN, model: anthropic.messages(MODEL_ID), prompt: PROMPT })).text,
  },
  {
    name: 'Anthropic Messages stream',
    run: () =>
      collectText(
        streamText({ maxOutputTokens: MAX_OUTPUT_TOKEN, model: anthropic.messages(MODEL_ID), prompt: PROMPT }).textStream,
      ),
  },
  {
    name: 'Anthropic tool round trip',
    run: () => toolRoundTrip(anthropic.messages(MODEL_ID)),
  },
  {
    name: 'Gemini generateContent',
    run: async () =>
      (await generateText({ maxOutputTokens: MAX_OUTPUT_TOKEN, model: google.chat(MODEL_ID), prompt: PROMPT })).text,
  },
  {
    name: 'Gemini streamGenerateContent',
    run: () =>
      collectText(streamText({ maxOutputTokens: MAX_OUTPUT_TOKEN, model: google.chat(MODEL_ID), prompt: PROMPT }).textStream),
  },
  {
    name: 'Gemini tool round trip',
    run: () => toolRoundTrip(google.chat(MODEL_ID)),
  },
];

let failures = 0;
for (const example of examples) {
  try {
    console.log(`OK   ${example.name}: ${await example.run()}`);
  } catch (error) {
    failures += 1;
    console.error(`FAIL ${example.name}: ${error instanceof Error ? error.message : String(error)}`);
  }
}

if (failures > 0) {
  console.error(`${failures} Vercel AI example(s) failed`);
  process.exitCode = 1;
}
