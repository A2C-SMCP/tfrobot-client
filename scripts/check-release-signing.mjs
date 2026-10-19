import { spawnSync } from 'node:child_process';
import { createHash, createPublicKey, verify } from 'node:crypto';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';

const require = createRequire(import.meta.url);
const probe = Buffer.from('TFRobot release signing preflight; not an application update.\n');

// Tauri updater keys/signatures are base64-wrapped Minisign boxes. Verify both
// the probe signature and trusted comment, using Node's Ed25519 implementation.
// Format: https://jedisct1.github.io/minisign/
function verifyProbe(encodedPublicKey, encodedSignature) {
  const publicLines = Buffer.from(encodedPublicKey, 'base64').toString('utf8').trim().split(/\r?\n/);
  const signatureLines = Buffer.from(encodedSignature, 'base64').toString('utf8').trim().split(/\r?\n/);
  const publicPacket = Buffer.from(publicLines[1] ?? '', 'base64');
  const signaturePacket = Buffer.from(signatureLines[1] ?? '', 'base64');
  const globalSignature = Buffer.from(signatureLines[3] ?? '', 'base64');
  if (publicPacket.length !== 42 || publicPacket.subarray(0, 2).toString() !== 'Ed'
    || signaturePacket.length !== 74 || globalSignature.length !== 64
    || !signatureLines[2]?.startsWith('trusted comment: ')) return false;
  if (!publicPacket.subarray(2, 10).equals(signaturePacket.subarray(2, 10))) return false;
  const algorithm = signaturePacket.subarray(0, 2).toString();
  if (algorithm !== 'Ed' && algorithm !== 'ED') return false;
  const publicKey = createPublicKey({
    key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), publicPacket.subarray(10)]),
    format: 'der', type: 'spki',
  });
  const signature = signaturePacket.subarray(10);
  const payload = algorithm === 'ED' ? createHash('blake2b512').update(probe).digest() : probe;
  return verify(null, payload, publicKey, signature)
    && verify(null, Buffer.concat([signature, Buffer.from(signatureLines[2].slice(17))]), publicKey, globalSignature);
}

let temporaryRoot;
try {
  if (!process.env.TAURI_SIGNING_PRIVATE_KEY) throw new Error('TAURI_SIGNING_PRIVATE_KEY is missing.');
  const configPath = resolve(process.argv[2] ?? 'src-tauri/tauri.conf.json');
  let publicKey;
  try {
    publicKey = JSON.parse(readFileSync(configPath, 'utf8')).plugins?.updater?.pubkey;
    if (typeof publicKey !== 'string' || !publicKey) throw new Error();
    // Mirror Tauri's support for a public-key file relative to src-tauri.
    const publicKeyPath = resolve(dirname(configPath), publicKey);
    if (existsSync(publicKeyPath)) publicKey = readFileSync(publicKeyPath, 'utf8');
  } catch {
    throw new Error('Unable to read the configured updater public key.');
  }
  temporaryRoot = mkdtempSync(join(tmpdir(), 'tfrc-signing-preflight-'));
  const probePath = join(temporaryRoot, 'probe.txt');
  writeFileSync(probePath, probe, { mode: 0o600 });
  const result = spawnSync(process.execPath, [require.resolve('@tauri-apps/cli/tauri.js'), 'signer', 'sign', probePath], {
    encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], timeout: 30_000,
    env: { ...process.env, TAURI_SIGNING_PRIVATE_KEY_PASSWORD: process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? '' },
  });
  // Never forward signer output: malformed credentials must not enter CI logs.
  if (result.status !== 0) {
    throw new Error('Unable to unlock updater signing key. Check TAURI_SIGNING_PRIVATE_KEY and its matching TAURI_SIGNING_KEY_PASSWORD repository secret.');
  }
  if (!verifyProbe(publicKey, readFileSync(`${probePath}.sig`, 'utf8'))) {
    throw new Error('Updater signing key does not match the configured public key, or its probe signature is invalid.');
  }
  console.log('Updater signing preflight passed: probe signature matches the configured public key.');
} catch (error) {
  // Only our fixed, non-sensitive diagnostics are emitted. Unexpected failures
  // (for example a missing .sig file) must also fail without printing raw input.
  const safeMessages = [
    'TAURI_SIGNING_PRIVATE_KEY is missing.', 'Unable to read the configured updater public key.',
    'Unable to unlock updater signing key.', 'Updater signing key does not match the configured public key,',
  ];
  const message = safeMessages.some((prefix) => error.message.startsWith(prefix))
    ? error.message : 'Updater signing preflight failed. Check the configured key, password, and public key.';
  console.error(message);
  process.exitCode = 1;
} finally {
  if (temporaryRoot) rmSync(temporaryRoot, { recursive: true, force: true });
}
