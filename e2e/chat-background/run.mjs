import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import { EventEmitter } from 'node:events';

const root = path.dirname(fileURLToPath(import.meta.url));
const out = path.resolve(process.env.BACKGROUND_OUTPUT ?? path.join(root, 'output-' + Date.now()));
const python = process.env.BACKGROUND_PYTHON;
const binary = process.env.BACKGROUND_BINARY;
assert(python && binary, 'Set BACKGROUND_PYTHON and BACKGROUND_BINARY; see README.md');
fs.mkdirSync(out, { recursive: true });
for (const name of ['run.mjs', 'server.py', 'page.tsx']) fs.copyFileSync(path.join(root, name), path.join(out, name));
fs.copyFileSync(path.join(root, '../../src-tauri/tauri.conf.json'), path.join(out, 'production-window-config.json'));
const events = new EventEmitter();
const rows = [];
const children = [];
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
function record(row) {
  rows.push(row);
  fs.appendFileSync(path.join(out, 'events.jsonl'), JSON.stringify(row) + '\n');
  events.emit('row', row);
  if (['stage', 'fault', 'connect', 'disconnect', 'native_action', 'failure', 'pass'].includes(row.kind))
    console.log(JSON.stringify(row));
}
function launch(command, args, source) {
  const child = spawn(command, args, { stdio: ['pipe', 'pipe', 'pipe'] });
  children.push(child);
  const log = fs.createWriteStream(path.join(out, source + '.log'));
  child.stderr.pipe(log);
  createInterface({ input: child.stdout }).on('line', line => {
    try { record({ ...JSON.parse(line), source }); } catch { log.write(line + '\n'); }
  });
  child.on('error', error => record({ kind: 'child_error', source, error: String(error) }));
  child.on('exit', code => record({ kind: 'child_exit', source, code }));
  return child;
}
function waitFor(predicate, after = 0, timeout = 15000) {
  const found = rows.slice(after).find(predicate);
  if (found) return Promise.resolve(found);
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => finish(new Error('Timed out waiting for event')), timeout);
    const listener = row => {
      if (predicate(row)) finish(null, row);
      else if (row.kind === 'child_error' || row.kind === 'child_exit') finish(new Error(JSON.stringify(row)));
    };
    function finish(error, row) {
      clearTimeout(timer); events.off('row', listener);
      if (error) reject(error); else resolve(row);
    }
    events.on('row', listener);
  });
}
async function post(route) {
  const response = await fetch('http://127.0.0.1:18766' + route, { method: 'POST', signal: AbortSignal.timeout(10000) });
  assert(response.ok, `${route}: ${response.status}`);
  return response.json();
}
let native;
async function action(name) {
  const start = rows.length;
  native.stdin.write(name + '\n');
  const result = await waitFor(row => row.kind === 'native_action' && row.action === name, start);
  assert.equal(result.ok, true);
}
async function snapshot(name) {
  const start = rows.length;
  await action(name);
  return waitFor(row => row.kind === 'snapshot' && row.name === name, start);
}
try {
  launch(python, [path.join(root, 'server.py')], 'server');
  await waitFor(row => row.kind === 'server_ready');
  native = launch(path.resolve(binary), [], 'native');
  await waitFor(row => row.kind === 'native_ready');
  try { await waitFor(row => row.kind === 'join'); } catch (error) { await snapshot('startup-failure'); throw error; }
  await waitFor(row => row.kind === 'snapshot' && row.name === 'render:INITIAL-HISTORY');
  const rounds = process.env.BACKGROUND_SMOKE === '1' ? ['hide'] : ['hide', 'minimize', 'hide', 'minimize'];
  for (const [index, mode] of rounds.entries()) {
    await action('show');
    await delay(1500);
    await action(mode);
    // Allow the OS minimize animation to finish before measuring the full 8 minutes.
    await delay(2000);
    const name = `round-${index + 1}-${mode}`;
    await post('/stage/' + name);
    const start = rows.length;
    const seconds = process.env.BACKGROUND_SMOKE === '1' ? 5 : 480;
    // Pure idle heartbeats: no measurement IPC, UI eval, or traffic generator here.
    await delay(seconds * 1000);
    assert(!rows.slice(start).some(row => ['disconnect', 'ping_timeout', 'child_exit'].includes(row.kind)), name + ': background connection failed');
    if (seconds === 480) {
      const heartbeats = rows.slice(start).filter(row => ['ping', 'pong'].includes(row.kind));
      // Engine.IO waits pingInterval AFTER the last PONG. A delayed but timely
      // PONG therefore reduces the count; 18 assumes nearly zero response time.
      assert(heartbeats.filter(row => row.kind === 'pong').length >= 5, name + ': insufficient heartbeat evidence');
      let pending;
      for (const row of heartbeats) {
        if (row.kind === 'ping') {
          assert(!pending, name + ': unanswered prior PING');
          pending = row;
        } else if (pending) {
          assert(row.mono - pending.mono < 60000, name + ': heartbeat response exceeded timeout');
          pending = undefined;
        }
      }
      if (pending) assert(Date.now() - pending.ts < 60000, name + ': pending heartbeat timed out');
    }
    if (index === rounds.length - 1) break; // Inject fault below while still long-backgrounded.
    const renderStart = rows.length;
    await post('/message/LIVE-' + name);
    const result = await waitFor(row => row.kind === 'snapshot' && row.name === 'render:LIVE-' + name, renderStart);
    assert.equal(result.visibility, 'hidden');
    assert(result.text.includes('LIVE-' + name), name + ': live message absent');
    await action('show');
    await delay(1000);
    assert((await snapshot(name + '-visible')).text.includes('LIVE-' + name));
  }
  await post('/stage/fault-background');
  const start = rows.length;
  await post('/fault');
  const injected = await waitFor(row => row.kind === 'fault', start);
  const connected = await waitFor(row => row.kind === 'connect', start, 15000);
  assert(connected.mono - injected.mono <= 15000, 'Background reconnect exceeded 15 seconds');
  await waitFor(row => row.kind === 'history' && row.ids.includes(injected.id), start);
  const recovered = await waitFor(row => row.kind === 'snapshot' && row.name === 'render:MISSED-WHILE-OFFLINE', start);
  assert.equal(recovered.visibility, 'hidden');
  assert(recovered.text.includes('MISSED-WHILE-OFFLINE'), 'Reconnected but missing REST-restored message');
  const liveStart = rows.length;
  await post('/message/LIVE-AFTER-RECOVERY');
  const live = await waitFor(row => row.kind === 'snapshot' && row.name === 'render:LIVE-AFTER-RECOVERY', liveStart);
  assert.equal(live.visibility, 'hidden');
  await action('show');
  await delay(1000);
  const final = await snapshot('final');
  assert.equal(final.text.split('MISSED-WHILE-OFFLINE').length - 1, 1, 'Recovered message duplicated');
  assert(!final.observations.some(row => ['error', 'rejection'].includes(row.kind)), 'Unhandled page error');
  const result = { kind: 'pass', smoke: process.env.BACKGROUND_SMOKE === '1', rounds: rounds.length,
    reconnectMs: connected.mono - injected.mono, restoredMessage: injected.id, out };
  record(result);
  fs.writeFileSync(path.join(out, 'result.json'), JSON.stringify(result, null, 2));
} catch (error) {
  record({ kind: 'failure', error: String(error) });
  process.exitCode = 1;
} finally {
  if (native && native.exitCode === null) native.stdin.end('quit\n');
  await delay(1000);
  for (const child of children) if (child.exitCode === null) child.kill('SIGTERM');
}
