import { generateText } from 'ai';

import { anthropic, CHAT_MODEL, google, openai } from './providers.ts';
import type { Example } from './runner.ts';

const PROMPT = 'I am sad.';

function requireTrace(trace: string | undefined): string {
  if (trace === undefined || trace.length === 0) {
    throw new Error('response omitted the requested reasoning trace');
  }
  return trace;
}

async function openAITrace(): Promise<string> {
  const result = await generateText({
    model: openai.responses(CHAT_MODEL),
    prompt: PROMPT,
    maxOutputTokens: 128,
    providerOptions: {
      openai: { forceReasoning: true, reasoningEffort: 'high', reasoningSummary: 'auto' },
    },
  });
  return requireTrace(result.reasoningText);
}

async function anthropicTrace(): Promise<string> {
  const result = await generateText({
    model: anthropic.messages(CHAT_MODEL),
    prompt: PROMPT,
    maxOutputTokens: 128,
    providerOptions: {
      anthropic: { thinking: { type: 'enabled', budgetTokens: 1024 } },
    },
  });
  return requireTrace(result.reasoningText);
}

async function geminiTrace(): Promise<string> {
  const result = await generateText({
    model: google.chat(CHAT_MODEL),
    prompt: PROMPT,
    maxOutputTokens: 128,
    providerOptions: {
      google: { thinkingConfig: { includeThoughts: true } },
    },
  });
  return requireTrace(result.reasoningText);
}

export const reasoningExamples: Example[] = [
  { name: 'OpenAI Responses reasoning', run: openAITrace },
  { name: 'Anthropic reasoning', run: anthropicTrace },
  { name: 'Gemini reasoning', run: geminiTrace },
];
