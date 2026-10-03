import { generateImage, generateText } from 'ai';

import { google, IMAGE_MODEL, openai } from './providers.ts';
import type { Example } from './runner.ts';

const PROMPT = 'A neon terminal dreaming in magenta and cyan.';

async function openAIImage(): Promise<string> {
  const result = await generateImage({
    model: openai.image(IMAGE_MODEL),
    prompt: PROMPT,
    size: '256x256',
  });
  return `${result.image.mediaType}, ${result.image.uint8Array.length} bytes`;
}

async function geminiImage(): Promise<string> {
  const result = await generateText({
    model: google.chat(IMAGE_MODEL),
    prompt: PROMPT,
    providerOptions: { google: { responseModalities: ['IMAGE'] } },
  });
  const image = result.files[0];
  if (image === undefined) {
    throw new Error('response omitted the generated image');
  }
  return `${image.mediaType}, ${image.uint8Array.length} bytes`;
}

export const imageExamples: Example[] = [
  { name: 'OpenAI image generation', run: openAIImage },
  { name: 'Gemini image generation', run: geminiImage },
];
