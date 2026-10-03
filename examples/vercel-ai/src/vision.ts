import { generateText } from 'ai';
import type { LanguageModel } from 'ai';

import { anthropic, CHAT_MODEL, geminiOpenAI, google, openai } from './providers.ts';
import type { Example } from './runner.ts';

const IMAGE = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAIAAAABAQAAAADcWUInAAAACklEQVQI12NwAAAAQgBBg7nsrQAAAABJRU5ErkJggg==',
  'base64',
);

const VISION_MODELS = [
  ['OpenAI Chat image input', openai.chat(CHAT_MODEL)],
  ['OpenAI Responses image input', openai.responses(CHAT_MODEL)],
  ['Gemini OpenAI-compatible image input', geminiOpenAI(CHAT_MODEL)],
  ['Anthropic image input', anthropic.messages(CHAT_MODEL)],
  ['Gemini image input', google.chat(CHAT_MODEL)],
] satisfies ReadonlyArray<readonly [string, LanguageModel]>;

async function inspectImage(model: LanguageModel): Promise<string> {
  const result = await generateText({
    model,
    maxOutputTokens: 128,
    messages: [
      {
        role: 'user',
        content: [
          { type: 'text', text: 'What can you measure in this image?' },
          { type: 'file', data: IMAGE, mediaType: 'image/png' },
        ],
      },
    ],
  });
  return result.text;
}

export const visionExamples: Example[] = VISION_MODELS.map(([name, model]) => ({
  name,
  run: () => inspectImage(model),
}));
