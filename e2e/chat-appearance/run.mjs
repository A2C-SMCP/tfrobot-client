import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { chromium } from '@playwright/test';
import { Server } from 'socket.io';

// Isolated public Kit HTTP/Socket fixture. No Manager, Tauri credentials or remote writes.
const output = await mkdtemp(join(tmpdir(), 'tfrc-chat-appearance-'));
const http = createServer((request, response) => {
  response.setHeader('Access-Control-Allow-Origin', '*');
  response.setHeader('Access-Control-Allow-Headers', 'authorization, content-type');
  if (request.method === 'OPTIONS') { response.writeHead(204); response.end(); return; }
  const path = new URL(request.url, 'http://fixture').pathname;
  const data = path.endsWith('/status') ? { working: false, task_id: null }
    : path.endsWith('/messages') ? {
      messages: [{ msgId: 'fixture-message', conversationId: 42,
        content: '你好！这是同一份数据下的聊天外观验收。\n\n消息与事件使用真实发布版 Chat Kit 渲染。',
        additionalKwargs: {}, attachments: null, createTimestamp: 1_800_000_000_000,
        creator: { uid: 'robot', name: 'Assistant', avatar: null }, role: 'assistant', msgType: 'text' }],
      events: [{ eventId: 'fixture-event', conversationId: 42, eventScene: 'Chain',
        status: 'success', createTimestamp: 1_800_000_000_000,
        content: '同源事件卡片：验证主题、详情入口与默认弹窗行为' }], cursor: null,
    } : { conversations: [{ conversationId: 42, title: '外观验收', updateTimestamp: 1_800_000_000_000 }], cursor: null };
  response.writeHead(200, { 'Content-Type': 'application/json' });
  response.end(JSON.stringify({ code: 200, message: 'Success', data }));
});
const sockets = new Server(http, { transports: ['websocket'], cors: { origin: '*' } });
sockets.of('/chat').on('connection', (socket) => socket.on('join_conversation', (_data, ack) => ack()));
let browser;
try {
  await new Promise((resolve, reject) => { http.once('error', reject); http.listen(18768, '127.0.0.1', resolve); });
  browser = await chromium.launch();
  const results = [];
  for (const dark of [false, true]) {
    for (const view of ['client', 'official']) {
      const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
      const errors = [];
      page.on('pageerror', (error) => errors.push(String(error)));
      await page.goto(`http://localhost:1420/e2e/chat-appearance/index.html?view=${view}&dark=${dark ? 1 : 0}`);
      await page.getByText('你好！这是同一份数据下的聊天外观验收。').waitFor();
      await page.getByRole('textbox', { name: '消息', exact: true }).waitFor();
      assert.equal(await page.getByRole('button', { name: /诊断 \(/ }).count(), 0);
      const trigger = page.getByRole('button', { name: /查看详情/ }).first();
      await trigger.click();
      await page.getByRole('dialog').waitFor();
      await page.keyboard.press('Escape');
      await page.getByRole('dialog').waitFor({ state: 'hidden' });
      const filename = `${view}-${dark ? 'dark' : 'light'}.png`;
      await page.screenshot({ path: join(output, filename), fullPage: true });
      const fontSize = await page.locator('.ant-app').last().evaluate((element) => getComputedStyle(element).fontSize);
      assert.equal(fontSize, '14px');
      assert.deepEqual(errors, []);
      results.push({ view, dark, fontSize, screenshot: filename });
      await page.close();
    }
  }
  await writeFile(join(output, 'result.json'), JSON.stringify({ passed: true, results,
    boundary: 'Same explicit host theme and fixture data; not full workspace parity or remote UAT.' }, null, 2));
  console.log(`PASS: ${output}`);
} finally {
  await browser?.close();
  await new Promise((resolve) => sockets.close(resolve));
  http.closeAllConnections();
  if (http.listening) await new Promise((resolve) => http.close(resolve));
}
