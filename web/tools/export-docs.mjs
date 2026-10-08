// Export public content only. Never copy the implementation tree or its history.
import { cp, mkdir, readFile, readdir, realpath, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { publicExamples } from './public-examples.mjs';

const [schemaURL, apiRoot, destination] = process.argv.slice(2);
if (!schemaURL || !apiRoot || !destination) {
  throw new Error('Usage: node web/tools/export-docs.mjs SCHEMA_URL API_ROOT OUTPUT_DIR');
}
const base = new URL(apiRoot);
if (!['http:', 'https:'].includes(base.protocol) || base.username || base.password || base.search || base.hash) {
  throw new Error('API_ROOT must be an HTTP(S) URL without credentials, query, or fragment');
}
const web = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const output = resolve(destination);
if (output === web || web.startsWith(`${output}/`) || output.startsWith(`${web}/`)) {
  throw new Error('OUTPUT_DIR must be outside the web source tree');
}
try {
  if ((await readdir(output)).length > 0) throw new Error('OUTPUT_DIR must be empty');
} catch (error) {
  if (error.code !== 'ENOENT') throw error;
}
const response = await fetch(schemaURL, { signal: AbortSignal.timeout(15_000) });
if (!response.ok) throw new Error(`OpenAPI fetch failed: ${response.status}`);
const schema = await response.json();
if (!schema.openapi?.startsWith('3.') || !schema.paths || !schema.info) {
  throw new Error('Expected an OpenAPI 3 document');
}
schema.servers = [{ url: apiRoot.replace(/\/$/, '') }];
schema.info.description = `${schema.info.description ?? ''}\nDevelopment documentation preview; hosted access is not live yet.`.trim();
// Root-level servers must also govern operations that supplied a local override.
for (const path of Object.values(schema.paths)) {
  delete path.servers;
  for (const operation of Object.values(path)) {
    if (operation && typeof operation === 'object') delete operation.servers;
  }
}
await mkdir(output, { recursive: true });
if (await realpath(output) !== output) throw new Error('OUTPUT_DIR must not use symlinks');
const docs = await readdir(join(web, 'content/docs'));
await mkdir(join(output, 'docs'), { recursive: true });
for (const name of docs.filter(name => name.endsWith('.md')).sort()) {
  const source = join(web, 'content/docs', name);
  if (await realpath(source) !== source) throw new Error(`Symlink rejected: ${name}`);
  await cp(source, join(output, 'docs', name));
}
for (const name of publicExamples) {
  const source = join(web, 'public/examples', name);
  if (await realpath(source) !== source) throw new Error(`Symlink rejected: ${name}`);
  await mkdir(dirname(join(output, 'examples', name)), { recursive: true });
  await cp(source, join(output, 'examples', name));
}
await writeFile(join(output, 'openapi.json'), `${JSON.stringify(schema, null, 2)}\n`);
await cp(join(web, 'PUBLIC-LICENSE'), join(output, 'LICENSE'));
await cp(join(output, 'docs/index.md'), join(output, 'README.md'));
// README links are rooted in docs; the source index is also retained there.
await writeFile(join(output, 'README.md'), (await readFile(join(output, 'README.md'), 'utf8'))
  .replace(/\]\(([a-z-]+\.md)\)/g, '](docs/$1)'));
await writeFile(join(output, 'llms.txt'), '# ELIZA API\n\nHosted access is not live yet.\n\n'
  + docs.filter(name => name.endsWith('.md')).sort().map(name => `- [${name}](docs/${name})`).join('\n') + '\n');
console.log(`Public docs exported to ${output}; API root: ${schema.servers[0].url}`);
