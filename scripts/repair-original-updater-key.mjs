import { createPrivateKey, createPublicKey, scryptSync, sign, verify } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
try {
 const original = process.env.TAURI_SIGNING_PRIVATE_KEY ?? '';
 const password = process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? '';
 if (password !== '') throw new Error();
 const lines = Buffer.from(original, 'base64').toString('utf8').trim().split(/\r?\n/);
 const packet = Buffer.from(lines[1] ?? '', 'base64');
 const configured = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8')).plugins.updater.pubkey;
 const pub = Buffer.from(Buffer.from(configured,'base64').toString('utf8').trim().split(/\r?\n/)[1], 'base64');
 if (packet.length !== 158 || packet.subarray(0,6).toString() !== 'EdScB2' || pub.length !== 42
   || packet.readBigUInt64LE(38) !== 1048576n || packet.readBigUInt64LE(46) !== 33554432n
   || !packet.subarray(54,62).equals(pub.subarray(2,10))
   || !packet.subarray(94,126).equals(pub.subarray(10))) throw new Error();
 const raw = createPrivateKey({key:Buffer.concat([Buffer.from('302e020100300506032b657004220420','hex'),packet.subarray(62,94)]),format:'der',type:'pkcs8'});
 if (!createPublicKey(raw).export({format:'der',type:'spki'}).subarray(-32).equals(pub.subarray(10))) throw new Error();
 const publicKey = createPublicKey({key:Buffer.concat([Buffer.from('302a300506032b6570032100','hex'),pub.subarray(10)]),format:'der',type:'spki'});
 const probe = Buffer.from('TFRobot original-key format repair; not an update.');
 if (!verify(null,probe,publicKey,sign(null,probe,raw))) throw new Error();
 // minisign 0.7.3: default opslimit=2^20, memlimit=32 MiB -> N=32768,r=8,p=1.
 // Older broken generators skipped this XOR for an empty password while retaining Sc.
 const stream = scryptSync(password,packet.subarray(6,38),104,{N:32768,r:8,p:1,maxmem:128*1024*1024});
 const repaired = Buffer.from(packet);
 for (let i=0;i<104;i++) repaired[54+i] ^= stream[i];
 const encoded = Buffer.from(`${lines[0]}\n${repaired.toString('base64')}\n`).toString('base64');
 const checked = spawnSync(process.execPath,['scripts/check-release-signing.mjs'],{encoding:'utf8',env:{...process.env,TAURI_SIGNING_PRIVATE_KEY:encoded},stdio:['ignore','pipe','pipe'],timeout:45000});
 if (checked.status !== 0) throw new Error();
 if (process.env.TFROBOT_REPAIR_TEST_ONLY === '1') {
  console.log('Original signing identity preserved; repaired serialization passes official CLI signing and cryptographic verification.');
 } else {
  const encrypted = spawnSync('python3',['scripts/seal-repaired-updater-key.py'],{encoding:'utf8',input:JSON.stringify({value:encoded}),stdio:['pipe','pipe','pipe'],timeout:10000});
  if (encrypted.status !== 0) throw new Error();
  const payload = JSON.parse(encrypted.stdout);
  if (typeof payload.encrypted_value !== 'string' || payload.key_id !== '3380204578043523366') throw new Error();
  writeFileSync(process.env.TFROBOT_SEALED_SECRET_OUTPUT,JSON.stringify(payload)+'\n',{mode:0o600});
  console.log('Original signing identity preserved and verified; only a repository-public-key-encrypted secret payload was exported.');
 }
} catch {console.error('Original-key repair refused or verification failed. No credentials were updated.');process.exitCode=1;}
