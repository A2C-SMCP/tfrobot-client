import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';

// Only booleans and classified outcomes leave the process. No key, password,
// signature, private-key fingerprint, or raw subprocess output is logged.
const privateKey = process.env.TAURI_SIGNING_PRIVATE_KEY ?? '';
const password = process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? '';
const box = Buffer.from(privateKey, 'base64').toString('utf8');
const encodedPayload = box.split(/\r?\n/).find((line) => line && !line.startsWith('untrusted comment:')) ?? '';
const payload = Buffer.from(encodedPayload, 'base64');
const probePath = join(tmpdir(), 'tfrobot-updater-diagnostic.txt');
writeFileSync(probePath, 'TFRobot updater signing diagnostic; not an application or update.\n');
const result = spawnSync('pnpm', ['exec', 'tauri', 'signer', 'sign', probePath], {
  encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
});
const response = `${result.stdout ?? ''}\n${result.stderr ?? ''}`;
let publicKeyMatchesSignature = null;
if (result.status === 0 && existsSync(`${probePath}.sig`)) {
  const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
  const publicBox = Buffer.from(config.plugins.updater.pubkey, 'base64').toString('utf8');
  const publicPayload = Buffer.from(publicBox.split(/\r?\n/)[1], 'base64');
  const signatureBox = Buffer.from(readFileSync(`${probePath}.sig`, 'utf8'), 'base64').toString('utf8');
  const signaturePayload = Buffer.from(signatureBox.split(/\r?\n/)[1], 'base64');
  publicKeyMatchesSignature = publicPayload.subarray(2, 10).equals(signaturePayload.subarray(2, 10));
}
console.log(JSON.stringify({
  checkedAt: new Date().toISOString(),
  privateKeyPresent: privateKey.length > 0,
  passwordEmpty: password.length === 0,
  privateBoxLooksValid: box.startsWith('untrusted comment:') && payload.length > 100,
  privateKeyUsesScrypt: payload.subarray(2, 4).toString('ascii') === 'Sc',
  signExitStatus: result.status,
  signSucceeded: result.status === 0,
  passwordRejected: /incorrect updater private key password|Wrong password for that key/.test(response),
  publicKeyMatchesSignature,
}));
