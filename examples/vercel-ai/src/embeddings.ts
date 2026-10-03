import { embed } from 'ai';
import type { EmbeddingModel } from 'ai';

import { EMBEDDING_MODEL, geminiOpenAI, google, openai } from './providers.ts';
import type { Example } from './runner.ts';

const EMBEDDING_MODELS = [
  ['OpenAI embeddings', openai.embedding(EMBEDDING_MODEL)],
  ['Gemini OpenAI-compatible embeddings', geminiOpenAI.embeddingModel(EMBEDDING_MODEL)],
  ['Gemini embeddings', google.embedding(EMBEDDING_MODEL)],
] satisfies ReadonlyArray<readonly [string, EmbeddingModel]>;

async function dimensions(model: EmbeddingModel): Promise<string> {
  const result = await embed({ model, value: 'People sometimes feel unhappy.' });
  return `${result.embedding.length} dimensions`;
}

export const embeddingExamples: Example[] = EMBEDDING_MODELS.map(([name, model]) => ({
  name,
  run: () => dimensions(model),
}));
