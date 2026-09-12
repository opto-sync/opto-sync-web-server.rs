// Proves the image argv contract: the service binary is pinned in ENTRYPOINT
// after the SOPS wrapper and CMD is empty, so extra runtime arguments
// (`docker run image --flag`, Kubernetes `args:`) are appended to the binary
// instead of replacing it. Run: node --test scripts/
//
// Optional image-level check: set CONTAINER_TEST_IMAGE=<built tag> to also
// assert the built image's Config.Entrypoint/Config.Cmd via `docker image inspect`.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const BINARY = '/usr/local/bin/opto-sync-web-server';
const WRAPPER = '/usr/local/bin/sops-entrypoint.sh';
const root = fileURLToPath(new URL('..', import.meta.url));
const wrapperSource = fileURLToPath(new URL('./sops-entrypoint.sh', import.meta.url));

// Minimal Dockerfile reader: joins continuations, drops comments, and returns
// the instructions of the final build stage (the default `docker build` target).
function finalStage(path) {
  const lines = readFileSync(path, 'utf8').split('\n');
  const instructions = [];
  let current = '';
  for (const raw of lines) {
    if (!current && /^\s*#/.test(raw)) continue;
    const line = raw.replace(/\s+$/, '');
    if (line.endsWith('\\')) { current += line.slice(0, -1) + ' '; continue; }
    current += line;
    if (current.trim()) instructions.push(current.trim());
    current = '';
  }
  const lastFrom = instructions.findLastIndex(i => /^FROM\s/i.test(i));
  assert.notEqual(lastFrom, -1, `${path}: no FROM`);
  return instructions.slice(lastFrom);
}
function execForm(stage, keyword) {
  const found = stage.filter(i => new RegExp(`^${keyword}\\s`, 'i').test(i));
  if (found.length === 0) return undefined;
  const value = found.at(-1).replace(new RegExp(`^${keyword}\\s+`, 'i'), '');
  let parsed;
  try { parsed = JSON.parse(value); } catch { assert.fail(`${keyword} must use JSON exec form, got: ${value}`); }
  assert.ok(Array.isArray(parsed) && parsed.every(s => typeof s === 'string'), `${keyword} must be a JSON string array`);
  return parsed;
}
// Docker semantics: run arguments replace CMD; ENTRYPOINT is kept. Kubernetes
// `args:` behaves the same way (`command:` would replace ENTRYPOINT).
function composeArgv(entrypoint, cmd, runArgs) {
  return [...entrypoint, ...(runArgs.length > 0 ? runArgs : cmd)];
}

const dockerfiles = ['Dockerfile', 'Dockerfile.arm64.dkf', 'Dockerfile.x86-64.dkf']
  .map(name => join(root, name)).filter(p => existsSync(p));

test('at least the default Dockerfile is present', () => {
  assert.ok(dockerfiles.some(p => p.endsWith('/Dockerfile')));
});
for (const path of dockerfiles) {
  test(`${path.slice(root.length)}: binary is pinned in ENTRYPOINT and CMD is empty`, () => {
    const stage = finalStage(path);
    assert.deepEqual(execForm(stage, 'ENTRYPOINT'), [WRAPPER, BINARY]);
    const cmd = execForm(stage, 'CMD');
    // ENTRYPOINT resets an inherited CMD, so an absent CMD is equivalent to [].
    assert.deepEqual(cmd ?? [], []);
    assert.deepEqual(composeArgv([WRAPPER, BINARY], cmd ?? [], ['--flag']), [WRAPPER, BINARY, '--flag']);
  });
}

// Execute the real wrapper with the argv Docker would build, mapping the image
// paths onto a fake service binary that records the arguments it received.
function runComposed(t, entrypoint, cmd, runArgs) {
  const dir = mkdtempSync(join(tmpdir(), 'argv-contract-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const fakeBinary = join(dir, 'service');
  writeFileSync(fakeBinary, `#!/bin/sh\nprintf 'SERVICE'\nfor a in "$@"; do printf ' [%s]' "$a"; done\n`, { mode: 0o755 });
  const argv = composeArgv(entrypoint, cmd, runArgs)
    .map(a => (a === BINARY ? fakeBinary : a === WRAPPER ? wrapperSource : a));
  assert.equal(argv[0], wrapperSource);
  // No ciphertext mounted, optional mode: the wrapper hands argv straight to exec.
  return spawnSync('/bin/sh', argv, {
    env: { PATH: '/usr/bin:/bin', SOPS_SECRETS_FILE: join(dir, 'absent'), SOPS_REQUIRE_KEY: '0' },
    encoding: 'utf8', timeout: 60000,
  });
}
test('extra runtime arguments reach the binary instead of replacing it', (t) => {
  const stage = finalStage(join(root, 'Dockerfile'));
  const entrypoint = execForm(stage, 'ENTRYPOINT');
  const cmd = execForm(stage, 'CMD') ?? [];
  const none = runComposed(t, entrypoint, cmd, []);
  assert.equal(none.status, 0, none.stderr);
  assert.equal(none.stdout, 'SERVICE');
  const extra = runComposed(t, entrypoint, cmd, ['--flag', 'value with space']);
  assert.equal(extra.status, 0, extra.stderr);
  assert.equal(extra.stdout, 'SERVICE [--flag] [value with space]');
});
test('negative control: the old ENTRYPOINT [wrapper] + CMD [binary] layout loses the binary', (t) => {
  // Guards the test itself: with the binary in CMD, `docker run image --flag`
  // makes the wrapper exec `--flag`, so the service never starts.
  const broken = runComposed(t, [WRAPPER], [BINARY], ['--flag']);
  assert.notEqual(broken.status, 0);
  assert.doesNotMatch(broken.stdout, /SERVICE/);
});
test('built image config matches the argv contract', { skip: !process.env.CONTAINER_TEST_IMAGE && 'CONTAINER_TEST_IMAGE not set' }, () => {
  const inspect = spawnSync('docker', ['image', 'inspect', '--format', '{{json .Config.Entrypoint}}|{{json .Config.Cmd}}', process.env.CONTAINER_TEST_IMAGE], { encoding: 'utf8', timeout: 60000 });
  assert.equal(inspect.status, 0, inspect.stderr);
  const [entrypoint, cmd] = inspect.stdout.trim().split('|').map(s => JSON.parse(s));
  assert.deepEqual(entrypoint, [WRAPPER, BINARY]);
  assert.deepEqual(cmd ?? [], []);
});
