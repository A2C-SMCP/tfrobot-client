import { createServer } from 'node:http';
import { Server } from 'socket.io';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import assert from 'node:assert/strict';
const directory = await mkdtemp(join(tmpdir(), 'chat-restoration-'));
const observations = [];
const http = createServer((request, response) => {
  const path = new URL(request.url, 'http://localhost').pathname;
  let data;
  if (path.endsWith('/status')) data = { working: false, task_id: null };
  else if (path.endsWith('/messages')) {
    const id = Number(path.match(/conversations\/(\d+)/)?.[1]);
    data = { messages: [{ msgId: `message-${id}`, conversationId: id, content: `History ${id}`,
      additionalKwargs: {}, attachments: null, createTimestamp: 1800000000000,
      creator: { uid: 'agent', name: 'Agent', avatar: null }, role: 'assistant', msgType: 'text' }], events: [], cursor: null };
  } else if (path.endsWith('/conversations')) data = { conversations: [42, 99].map(id => ({
    conversationId: id, title: `Conversation ${id}`, description: null, updateTimestamp: 1800000000000 - id,
  })), cursor: null };
  else { response.writeHead(404); response.end(); return; }
  response.writeHead(200, { 'Content-Type': 'application/json' });
  response.end(JSON.stringify({ code: 200, message: 'Success', data }));
});
const sockets = new Server(http, { transports: ['websocket'], cors: { origin: '*' } });
sockets.of('/chat').on('connection', socket => socket.on('join_conversation', (_data, ack) => ack()));
await new Promise(resolve => http.listen(18767, '127.0.0.1', resolve));
let running;
function launch(phase) {
  const child = spawn(resolve('src-tauri/target/debug/examples/chat_restoration_acceptance'), [], {
    env: { ...process.env, CHAT_RESTORE_DATA: directory }, stdio: ['pipe', 'pipe', 'pipe'],
  });
  running = child;
  const events = [];
  const waiters = new Set();
  const exited = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('exit', (code, signal) => { running = undefined; resolve({ code, signal }); });
  });
  createInterface({ input: child.stdout }).on('line', line => {
    if (!line.startsWith('ACCEPTANCE:')) return;
    const event = JSON.parse(line.slice('ACCEPTANCE:'.length));
    events.push(event); observations.push({ phase, ...event });
    process.stdout.write(`${phase} ${JSON.stringify(event)}\n`);
    for (const waiter of waiters) waiter();
  });
  child.stderr.on('data', data => process.stderr.write(data));
  const wait = (predicate) => new Promise((resolve, reject) => {
    const timer = setTimeout(() => { waiters.delete(check); reject(new Error(`Timed out in ${phase}: ${predicate}`)); }, 30000);
    function check() {
      const match = events.find(predicate);
      if (match) { clearTimeout(timer); waiters.delete(check); resolve(match); }
    }
    waiters.add(check); check();
  });
  return { events, wait, send: js => child.stdin.write(`${js}\n`), quit: async () => {
    child.stdin.write('quit\n'); const result = await exited; assert.equal(result.code, 0);
  } };
}
// Wait for DOM changes, never repeatedly query on a timer.
function clickWhen(selector, text, mouseDown = false) {
  return `(() => { const find = () => Array.from(document.querySelectorAll(${JSON.stringify(selector)})).find(e => ${text ? `e.textContent.includes(${JSON.stringify(text)})` : 'true'}); const click = () => { const e = find(); if (!e) return false; ${mouseDown ? "e.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));" : 'e.click();'} return true; }; if (!click()) { const o = new MutationObserver(() => { if (click()) o.disconnect(); }); o.observe(document.body, { childList: true, subtree: true }); } })();`;
}
try {
  const first = launch('first');
  await first.wait(e => e.kind === 'render' && e.marker === 'History 42');
  first.send(clickWhen('.ant-select-selector', '', true));
  first.send(clickWhen('.ant-select-item-option', 'Robot 43'));
  await first.wait(e => e.kind === 'saved' && e.employeeId === '43' && e.conversationId === '42');
  first.send(clickWhen('button[aria-expanded]', ''));
  first.send(clickWhen('[role="menuitem"]', 'Conversation 99'));
  await first.wait(e => e.kind === 'render' && e.marker === 'History 99');
  await first.wait(e => e.kind === 'saved' && e.employeeId === '43' && e.conversationId === '99');
  await first.quit();
  const second = launch('restart');
  await second.wait(e => e.kind === 'render' && e.marker === 'History 99');
  assert.deepEqual(second.events.filter(e => e.kind === 'opened').map(e => e.employeeId), [43]);
  assert(!second.events.some(e => e.kind === 'render' && e.marker === 'History 42'));
  await second.wait(e => e.kind === 'saved' && e.employeeId === '43' && e.conversationId === '99');
  await second.quit();
  await writeFile(join(directory, 'result.json'), JSON.stringify({ passed: true, observations }, null, 2));
  console.log(`PASS: ${directory}/result.json`);
} finally {
  if (running) running.stdin.write('quit\n');
  await new Promise(resolve => sockets.close(resolve));
  http.closeAllConnections();
}
