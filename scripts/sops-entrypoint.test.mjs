// Contract tests for scripts/sops-entrypoint.sh. Run: node --test scripts/
// Uses a fake `sops` on PATH; no real ciphertext or age identity is involved.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const wrapper = fileURLToPath(new URL('./sops-entrypoint.sh', import.meta.url));
const systemPrintenv = ['/usr/bin/printenv', '/bin/printenv'].find(p => existsSync(p));

function fixture(t, data = '') {
  const dir = mkdtempSync(join(tmpdir(), 'sops-contract-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  mkdirSync(join(dir, 'bin'));
  writeFileSync(join(dir, 'ciphertext'), 'SYNTHETIC_NOT_CIPHERTEXT');
  writeFileSync(join(dir, 'data'), data);
  writeFileSync(join(dir, 'bin/sops'), '#!/bin/sh\nif [ "${SOPS_TEST_FAIL:-0}" = 1 ]; then printf SYNTHETIC_PRIVATE_MARKER >&2; exit 9; fi\n/bin/cat "$SOPS_TEST_DATA"\n', { mode: 0o755 });
  const env = {
    PATH: `${dir}/bin:/usr/bin:/bin`, SOPS_SECRETS_FILE: join(dir, 'ciphertext'),
    SOPS_AGE_KEY: 'SYNTHETIC_NOT_AN_AGE_IDENTITY', SOPS_REQUIRE_KEY: '1',
    SOPS_TEST_DATA: join(dir, 'data'),
  };
  return { dir, env };
}
function run(f, args = [process.execPath, '-e', 'process.stdout.write("started")'], env = {}) {
  const result = spawnSync('/bin/sh', [wrapper, ...args], {
    env: { ...f.env, ...env }, encoding: 'utf8', timeout: 30000, maxBuffer: 256 * 1024,
  });
  assert.ifError(result.error);
  return result;
}
function failure(result, code, message) {
  assert.equal(result.status, code);
  assert.equal(result.stdout, '');
  assert.equal(result.stderr, `sops-entrypoint: ${message}\n`);
}

test('POSIX syntax and no eval/source/plaintext heredoc', () => {
  assert.equal(spawnSync('/bin/sh', ['-n', wrapper]).status, 0);
  const source = readFileSync(wrapper, 'utf8').split('\n').filter(line => !line.trimStart().startsWith('#')).join('\n');
  assert.doesNotMatch(source, /\beval\b|\bsource\b|<<|^\s*\.\s/m);
});
for (const required of ['0', '1']) {
  for (const ciphertext of [false, true]) {
    for (const key of [false, true]) {
      test(`startup matrix required=${required} ciphertext=${ciphertext} identity=${key}`, (t) => {
        const f = fixture(t, 'EXAMPLE_VALUE=synthetic\n');
        const result = run(f, undefined, {
          SOPS_REQUIRE_KEY: required,
          SOPS_SECRETS_FILE: join(f.dir, ciphertext ? 'ciphertext' : 'missing'),
          SOPS_AGE_KEY: key ? f.env.SOPS_AGE_KEY : '',
        });
        const denied = required === '1' && (!ciphertext || !key);
        assert.equal(result.status, denied ? 1 : 0);
        assert.equal(result.stdout, denied ? '' : 'started');
        assert.doesNotMatch(result.stderr, /SYNTHETIC|sops-contract-/);
      });
    }
  }
}
for (const value of ['', 'two words', 'a=b==', '$(printf forbidden)', '`printf forbidden`', '"quoted"', 'back\\slash', '*']) {
  test(`decrypted value stays literal: ${JSON.stringify(value)}`, (t) => {
    const f = fixture(t, `EXAMPLE_VALUE=${value}\n`);
    const result = run(f, [process.execPath, '-e', 'process.stdout.write(JSON.stringify(process.env.EXAMPLE_VALUE))']);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(JSON.parse(result.stdout), value);
    assert.equal(result.stderr, '');
  });
}
for (const value of ['', 'orchestrator']) {
  test(`orchestrator precedence includes ${JSON.stringify(value)}`, (t) => {
    const f = fixture(t, 'EXAMPLE_VALUE=decrypted\n');
    const result = run(f, [process.execPath, '-e', 'process.stdout.write(JSON.stringify(process.env.EXAMPLE_VALUE))'], { EXAMPLE_VALUE: value });
    assert.equal(result.status, 0);
    assert.equal(JSON.parse(result.stdout), value);
  });
}
for (const [data, message] of [
  ['GOOD=synthetic\ninvalid record\n', 'invalid dotenv record'],
  ['A=one\nA=two\n', 'duplicate variable name'],
  ['=synthetic\n', 'invalid or reserved variable name'],
  ['9BAD=synthetic\n', 'invalid or reserved variable name'],
  ['BAD-NAME=synthetic\n', 'invalid or reserved variable name'],
  ['_ORES_SOPS_IMPORT=synthetic\n', 'invalid or reserved variable name'],
]) {
  test(`malformed payload fails closed: ${JSON.stringify(data)}`, (t) => {
    failure(run(fixture(t, data)), 1, message);
  });
}

// --- reserved shell / dynamic-loader / libc / wrapper control names ---------
const RESERVED = [
  'PATH', 'IFS', 'CDPATH', 'ENV', 'BASH_ENV', 'BASH_FUNC_x%%', 'SHELLOPTS', 'BASHOPTS', 'GLOBIGNORE', 'PS4', 'POSIXLY_CORRECT',
  'LD_PRELOAD', 'LD_LIBRARY_PATH', 'LD_AUDIT', 'LD_DEBUG', 'LD_PROFILE', 'LD_BIND_NOW', 'DYLD_INSERT_LIBRARIES', 'DYLD_LIBRARY_PATH',
  'GCONV_PATH', 'GETCONF_DIR', 'HOSTALIASES', 'LOCALDOMAIN', 'LOCPATH', 'MALLOC_TRACE', 'GLIBC_TUNABLES', 'NIS_PATH', 'NLSPATH',
  'RESOLV_HOST_CONF', 'RES_OPTIONS', 'TMPDIR', 'TZDIR',
  'SOPS_SECRETS_FILE', 'SOPS_AGE_KEY', 'SOPS_AGE_KEY_FILE', 'SOPS_REQUIRE_KEY', 'Sops_Age_Key',
];
for (const name of RESERVED.filter(n => /^[A-Za-z_][A-Za-z0-9_]*$/.test(n))) {
  test(`reserved name is refused before any export or application start: ${name}`, (t) => {
    // Put the reserved record after an ordinary one to prove nothing is exported
    // first, and use a canary application that would report if it ever ran.
    const f = fixture(t, `ORDINARY=synthetic\n${name}=/SYNTHETIC_REDIRECT\n`);
    const result = run(f, [process.execPath, '-e', 'process.stdout.write("APPLICATION_STARTED")']);
    failure(result, 1, 'reserved variable name');
    assert.doesNotMatch(result.stderr, /SYNTHETIC|sops-contract-/);
  });
}
test('names with shell-function syntax (BASH_FUNC_x%%) are rejected as invalid', (t) => {
  failure(run(fixture(t, 'BASH_FUNC_x%%=() { :; }\n')), 1, 'invalid or reserved variable name');
});
test('lowercase sops_* metadata is skipped, not imported', (t) => {
  const f = fixture(t, 'sops_age_key=metadata\nNEXT=works\n');
  const result = run(f, [process.execPath, '-e', 'process.stdout.write(JSON.stringify([process.env.sops_age_key ?? null, process.env.NEXT]))']);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), [null, 'works']);
});
test('decrypted PATH cannot redirect helper lookup', (t) => {
  const f = fixture(t);
  mkdirSync(join(f.dir, 'evil'));
  writeFileSync(join(f.dir, 'evil/printenv'), '#!/bin/sh\nexit 77\n', { mode: 0o755 });
  writeFileSync(join(f.dir, 'evil/sops'), '#!/bin/sh\nexit 78\n', { mode: 0o755 });
  writeFileSync(join(f.dir, 'data'), `A=1\nPATH=${join(f.dir, 'evil')}\nB=2\n`);
  failure(run(f), 1, 'reserved variable name');
});
test('decrypted LD_PRELOAD cannot inject a loader into the application', (t) => {
  const f = fixture(t, `LD_PRELOAD=${'/SYNTHETIC/does-not-exist.so'}\n`);
  const result = run(f, [process.execPath, '-e', 'process.stdout.write(String(process.env.LD_PRELOAD))']);
  failure(result, 1, 'reserved variable name');
});
test('decrypted SOPS_* cannot change the current file/key policy', (t) => {
  // A payload that tries to disable the requirement or re-point the ciphertext
  // is refused; the policy the wrapper started with is the one enforced.
  for (const data of ['SOPS_REQUIRE_KEY=0\n', 'SOPS_SECRETS_FILE=/dev/null\n', 'SOPS_AGE_KEY_FILE=/SYNTHETIC\n']) {
    failure(run(fixture(t, data)), 1, 'reserved variable name');
  }
});
test('reserved-looking but ordinary names are still imported', (t) => {
  const names = ['PATHS', 'MY_PATH', 'LDAP_URL', 'ENVIRONMENT', 'APP_ENV', 'IFS_MODE', 'TMPDIR_HINT', 'SOPSY'];
  const f = fixture(t, names.map(n => `${n}=v-${n}`).join('\n') + '\n');
  const result = run(f, [process.execPath, '-e', `process.stdout.write(JSON.stringify(${JSON.stringify(names)}.map(n=>process.env[n])))`]);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), names.map(n => `v-${n}`));
});
test('inherited orchestrator PATH/LD_* are untouched (only decrypted data is policed)', (t) => {
  const f = fixture(t, 'EXAMPLE_VALUE=synthetic\n');
  const result = run(f, [process.execPath, '-e', 'process.stdout.write(String(process.env.LD_BIND_NOW))'], { LD_BIND_NOW: '1' });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, '1');
});
test('age identity is removed from the application environment', (t) => {
  const f = fixture(t, 'EXAMPLE_VALUE=synthetic\n');
  for (const extra of [{}, { SOPS_AGE_KEY: '', SOPS_AGE_KEY_FILE: join(f.dir, 'ciphertext') }]) {
    const result = run(f, [process.execPath, '-e', 'process.stdout.write(JSON.stringify([process.env.SOPS_AGE_KEY ?? null, process.env.SOPS_AGE_KEY_FILE ?? null, process.env.EXAMPLE_VALUE]))'], extra);
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(JSON.parse(result.stdout), [null, null, 'synthetic']);
  }
});
// -----------------------------------------------------------------------------

test('comments, metadata, final line without newline, and internal-name lookalikes', (t) => {
  const f = fixture(t, '# comment\n\nsops_metadata=ignored\nkey=one\nvalue=two\nsecrets=three');
  const result = run(f, [process.execPath, '-e', 'process.stdout.write(JSON.stringify([process.env.key,process.env.value,process.env.secrets,Object.keys(process.env).filter(k=>k.startsWith("_ORES_SOPS_"))]))']);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), ['one', 'two', 'three', []]);
});
test('exports cannot alter helpers during the presence-check phase', (t) => {
  const f = fixture(t, 'EXPORTED_CANARY=synthetic\nNEXT=works\n');
  writeFileSync(join(f.dir, 'bin/printenv'), `#!/bin/sh\nif [ -n "\${EXPORTED_CANARY:-}" ]; then exit 77; fi\nexec ${systemPrintenv} "$@"\n`, { mode: 0o755 });
  const result = run(f, [process.execPath, '-e', 'process.stdout.write(process.env.NEXT)']);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, 'works');
});
test('decryption failure is sanitized and never starts the application', (t) => {
  failure(run(fixture(t), undefined, { SOPS_TEST_FAIL: '1' }), 1, 'decryption failed');
});
test('environment-check errors are not treated as unset', (t) => {
  const f = fixture(t, 'EXAMPLE_VALUE=synthetic\n');
  writeFileSync(join(f.dir, 'bin/printenv'), '#!/bin/sh\nexit 77\n', { mode: 0o755 });
  failure(run(f), 1, 'environment presence check failed');
});
test('ciphertext directories are rejected even in optional mode', (t) => {
  const f = fixture(t);
  failure(run(f, undefined, { SOPS_REQUIRE_KEY: '0', SOPS_SECRETS_FILE: f.dir }), 1, 'required ciphertext is unavailable or not a regular file');
});
test('invalid required mode is rejected without value reflection', (t) => {
  failure(run(fixture(t), undefined, { SOPS_REQUIRE_KEY: 'SYNTHETIC_PRIVATE_MARKER' }), 64, 'invalid required-key setting');
});
for (const args of [[], ['']]) {
  test(`missing command fails before secret handling: ${args.length}`, (t) => {
    failure(run(fixture(t), args), 64, 'no command configured');
  });
}
for (const status of [0, 23, 64, 126, 127]) {
  test(`preserves application exit ${status}`, (t) => {
    assert.equal(run(fixture(t), [process.execPath, '-e', `process.exit(${status})`]).status, status);
  });
}
test('successful decryption preserves application PID and SIGTERM delivery', { timeout: 60000 }, async (t) => {
  const f = fixture(t, 'EXAMPLE_VALUE=synthetic\n');
  const child = spawn('/bin/sh', [wrapper, process.execPath, '-e',
    'process.on("SIGTERM",()=>process.exit(42)); process.stdout.write(process.pid+"\\n"); setInterval(()=>{},1000)'],
    { env: f.env, stdio: ['ignore', 'pipe', 'pipe'] });
  t.after(() => { if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL'); });
  const exited = new Promise((resolve, reject) => {
    child.once('error', reject); child.once('exit', (code, signal) => resolve({ code, signal }));
  });
  const pid = await new Promise((resolve, reject) => {
    let output = '';
    child.once('error', reject);
    child.stdout.on('data', data => { output += data; if (output.includes('\n')) resolve(Number(output.trim())); });
    child.once('exit', () => reject(new Error('application exited before readiness')));
  });
  assert.equal(pid, child.pid);
  child.kill('SIGTERM');
  assert.deepEqual(await exited, { code: 42, signal: null });
});
test('inherited loader variables cannot export the plaintext buffer to helpers', (t) => {
  const f = fixture(t, 'EXAMPLE_VALUE=synthetic\n');
  writeFileSync(join(f.dir, 'bin/printenv'), `#!/bin/sh\nif [ "\${_ORES_SOPS_PLAINTEXT+x}" = x ]; then exit 77; fi\nexec ${systemPrintenv} "$@"\n`, { mode: 0o755 });
  const result = run(f, undefined, { _ORES_SOPS_PLAINTEXT: 'SYNTHETIC_CONTROL_INPUT' });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, 'started');
});
