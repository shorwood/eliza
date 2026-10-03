import { generateSpeech } from 'ai';

import { google, openai, SPEECH_MODEL } from './providers.ts';
import type { Example } from './runner.ts';

const TEXT = 'Hello from ELIZA.';

async function speech(model: Parameters<typeof generateSpeech>[0]['model']): Promise<string> {
  const result = await generateSpeech({ model, text: TEXT, voice: 'retro', outputFormat: 'wav' });
  return `${result.audio.mediaType}, ${result.audio.uint8Array.length} bytes`;
}

export const speechExamples: Example[] = [
  { name: 'OpenAI speech', run: () => speech(openai.speech(SPEECH_MODEL)) },
  { name: 'Gemini speech', run: () => speech(google.speech(SPEECH_MODEL)) },
];
