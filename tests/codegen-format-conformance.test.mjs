import { readFile } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parse } from '@puppeteer/replay';

const fixtureRoot = new URL('../cli/src/native/codegen/test-fixtures/', import.meta.url);

test('the shared Recorder fixture passes the supported parser', async () => {
  const source = await readFile(new URL('flow.json', fixtureRoot), 'utf8');
  const flow = parse(JSON.parse(source));
  assert.equal(flow.title, 'hostile "flow"\nname');
  assert.equal(flow.steps.length, 5);
});

test('the shared Playwright fixture compiles and collects', async () => {
  const fixture = new URL('flow.spec.ts', fixtureRoot);
  const child = spawn(
    process.execPath,
    [new URL('../node_modules/@playwright/test/cli.js', import.meta.url).pathname, 'test', '--list', fixture.pathname],
    { cwd: new URL('..', import.meta.url), stdio: ['ignore', 'pipe', 'pipe'] },
  );
  let output = '';
  child.stdout.on('data', chunk => { output += chunk; });
  child.stderr.on('data', chunk => { output += chunk; });
  const exitCode = await new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', resolve);
  });
  assert.equal(exitCode, 0, output);
  assert.match(output, /hostile/);
});
