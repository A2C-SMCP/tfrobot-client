import { readFileSync } from 'node:fs';
import { createPrivateKey, createPublicKey, sign, verify } from 'node:crypto';
// Read-only recognition of upstream tauri#15022's unencrypted-key encoding bug.
// Credentials, private-key bytes and signatures never leave this process.
try {
 const box = Buffer.from(process.env.TAURI_SIGNING_PRIVATE_KEY ?? '', 'base64').toString('utf8');
 const packet = Buffer.from(box.trim().split(/\r?\n/)[1] ?? '', 'base64');
 const configured = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8')).plugins.updater.pubkey;
 const pub = Buffer.from(Buffer.from(configured,'base64').toString('utf8').trim().split(/\r?\n/)[1], 'base64');
 const correctFormat = packet.length === 158 && packet.subarray(0,6).toString() === 'EdScB2' && pub.length === 42;
 let rawSeedMatchesPublicKey = false, rawKeyIdMatches = false, rawStoredPublicMatches = false, signatureValid = false;
 if (correctFormat) {
  const key = createPrivateKey({key:Buffer.concat([Buffer.from('302e020100300506032b657004220420','hex'),packet.subarray(62,94)]),format:'der',type:'pkcs8'});
  const derived = createPublicKey(key).export({format:'der',type:'spki'}).subarray(-32);
  rawSeedMatchesPublicKey = derived.equals(pub.subarray(10));
  rawStoredPublicMatches = packet.subarray(94,126).equals(pub.subarray(10));
  rawKeyIdMatches = packet.subarray(54,62).equals(pub.subarray(2,10));
  const probe = Buffer.from('TFRobot existing-key recoverability check; not an update.');
  const publicKey = createPublicKey({key:Buffer.concat([Buffer.from('302a300506032b6570032100','hex'),pub.subarray(10)]),format:'der',type:'spki'});
  signatureValid = verify(null,probe,publicKey,sign(null,probe,key));
 }
 console.log(JSON.stringify({correctFormat,passwordEmpty:!process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD,rawSeedMatchesPublicKey,rawStoredPublicMatches,rawKeyIdMatches,signatureValid,recoverableWithoutChangingPublicKey:correctFormat&&rawSeedMatchesPublicKey&&rawStoredPublicMatches&&rawKeyIdMatches&&signatureValid}));
} catch { console.error('Read-only key-format check failed; no credentials were changed.'); process.exitCode=1; }
