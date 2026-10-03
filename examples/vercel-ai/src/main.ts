import { chatExamples } from './chat.ts';
import { embeddingExamples } from './embeddings.ts';
import { imageExamples } from './images.ts';
import { reasoningExamples } from './reasoning.ts';
import { runExamples } from './runner.ts';
import { speechExamples } from './speech.ts';
import { visionExamples } from './vision.ts';

await runExamples([
  ...chatExamples,
  ...reasoningExamples,
  ...visionExamples,
  ...embeddingExamples,
  ...imageExamples,
  ...speechExamples,
]);
