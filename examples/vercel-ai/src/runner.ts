export type Example = {
  name: string;
  run: () => Promise<string>;
};

export async function runExamples(examples: readonly Example[]): Promise<void> {
  let failures = 0;

  for (const example of examples) {
    try {
      console.log(`OK   ${example.name}: ${await example.run()}`);
    } catch (error) {
      failures += 1;
      const message = error instanceof Error ? error.message : String(error);
      console.error(`FAIL ${example.name}: ${message}`);
    }
  }

  if (failures > 0) {
    console.error(`${failures} Vercel AI example(s) failed`);
    process.exitCode = 1;
  }
}
