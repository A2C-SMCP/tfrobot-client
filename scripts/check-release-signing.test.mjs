import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const require = createRequire(import.meta.url);
const cli = require.resolve('@tauri-apps/cli/tauri.js');
const script = resolve('scripts/check-release-signing.mjs');
const password = 'generated-test-key-password';
let fixtureRoot;
let encryptedKey;
let emptyPasswordKey;

function generateKey(name, keyPassword) {
  const path = join(fixtureRoot, name);
  const result = spawnSync(process.execPath, [cli, 'signer', 'generate', '--ci', '-p', keyPassword, '-w', path], {
    encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
  });
  assert.equal(result.status, 0, 'Tauri must generate the disposable fixture key');
  return { path, privateKey: readFileSync(path, 'utf8'), publicKey: readFileSync(`${path}.pub`, 'utf8') };
}

function check(privateKey, keyPassword, publicKey) {
  const configPath = join(fixtureRoot, 'tauri.conf.json');
  writeFileSync(configPath, JSON.stringify({ plugins: { updater: { pubkey: publicKey } } }));
  const env = {
    ...process.env, CI: 'true', TAURI_SIGNING_PRIVATE_KEY: privateKey,
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: keyPassword,
  };
  delete env.TAURI_SIGNING_PRIVATE_KEY_PATH;
  return spawnSync(process.execPath, [script, configPath], {
    encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
    env,
  });
}

before(() => {
  fixtureRoot = mkdtempSync(join(tmpdir(), 'tfrc-signing-fixtures-'));
  encryptedKey = generateKey('encrypted.key', password);
  emptyPasswordKey = generateKey('empty-password.key', '');
});
after(() => rmSync(fixtureRoot, { recursive: true, force: true }));

test('signs with the real Tauri CLI and verifies the configured public key', () => {
  const result = check(encryptedKey.privateKey, password, encryptedKey.publicKey);
  assert.equal(result.status, 0, 'Matching signing credentials must pass');
  assert.match(result.stdout, /signing preflight passed/i);
});

test('supports a key intentionally created with an empty password', () => {
  assert.equal(check(emptyPasswordKey.privateKey, '', emptyPasswordKey.publicKey).status, 0);
});

test('supports Tauri private/public key file paths as well as inline keys', () => {
  assert.equal(check(encryptedKey.path, password, `${encryptedKey.path}.pub`).status, 0);
});

test('rejects the release regression: encrypted key with an incorrect empty password', () => {
  const result = check(encryptedKey.privateKey, '', encryptedKey.publicKey);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /unable to unlock updater signing key/i);
});

test('rejects a valid signing key that differs from the embedded public key', () => {
  const result = check(encryptedKey.privateKey, password, emptyPasswordKey.publicKey);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /does not match.*public key/i);
});

test('cryptographically rejects a different public key even when its key ID is copied', () => {
  const firstBox = Buffer.from(encryptedKey.publicKey, 'base64').toString('utf8').trim().split('\n');
  const otherBox = Buffer.from(emptyPasswordKey.publicKey, 'base64').toString('utf8').trim().split('\n');
  const otherPacket = Buffer.from(otherBox[1], 'base64');
  Buffer.from(firstBox[1], 'base64').copy(otherPacket, 2, 2, 10);
  otherBox[1] = otherPacket.toString('base64');
  const result = check(encryptedKey.privateKey, password, Buffer.from(otherBox.join('\n')).toString('base64'));
  assert.equal(result.status, 1);
  assert.match(result.stderr, /does not match.*public key/i);
});

test('rejects a missing signing key before invoking the signer', () => {
  const result = check('', '', encryptedKey.publicKey);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /TAURI_SIGNING_PRIVATE_KEY is missing/);
});

test('does not print supplied credentials or raw signer output on errors', () => {
  const canaryKey = 'PRIVATE-KEY-CANARY-DO-NOT-PRINT';
  const canaryPassword = 'PASSWORD-CANARY-DO-NOT-PRINT';
  const result = check(canaryKey, canaryPassword, encryptedKey.publicKey);
  assert.equal(result.status, 1);
  assert.equal(`${result.stdout}${result.stderr}`.includes(canaryKey), false);
  assert.equal(`${result.stdout}${result.stderr}`.includes(canaryPassword), false);
});
