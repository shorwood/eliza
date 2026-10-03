import { createAnthropic } from '@ai-sdk/anthropic';
import { createGoogle } from '@ai-sdk/google';
import { createOpenAI } from '@ai-sdk/openai';
import { createOpenAICompatible } from '@ai-sdk/openai-compatible';

const BASE_URL = process.env.ELIZA_BASE_URL ?? 'http://127.0.0.1:8787';

export const CHAT_MODEL = 'eliza-1966';
export const EMBEDDING_MODEL = 'fnv-embed';
export const IMAGE_MODEL = 'eliza-retro-image';
export const SPEECH_MODEL = 'flite';

export const openai = createOpenAI({
  apiKey: 'local',
  baseURL: `${BASE_URL}/openai/v1`,
});

export const geminiOpenAI = createOpenAICompatible({
  apiKey: 'local',
  baseURL: `${BASE_URL}/gemini/v1beta/openai`,
  name: 'eliza-gemini-openai',
});

export const anthropic = createAnthropic({
  apiKey: 'local',
  baseURL: `${BASE_URL}/anthropic/v1`,
});

export const google = createGoogle({
  apiKey: 'local',
  baseURL: `${BASE_URL}/gemini/v1beta`,
});
