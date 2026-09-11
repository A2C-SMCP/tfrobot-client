// Invoked by the explicit Rust browser test while its real HTTP fixtures remain alive.
import assert from 'node:assert/strict';
import { chromium } from '@playwright/test';

const urls = JSON.parse(process.env.CHAT_RESOURCE_TEST_URLS);
const browser = await chromium.launch({ headless: true });
try {
  const page = await browser.newPage();
  page.on('requestfailed', (request) => console.error('resource request failure', request.failure()?.errorText));
  page.on('console', (message) => { if (message.type() === 'error') console.error(message.text().replace(/http:\/\/127\.0\.0\.1:[^\s'"]+/g, '[local-resource]')); });
  page.on('response', (response) => { if (response.status() >= 400) console.error('resource HTTP', response.status()); });
  // Use a real loopback navigation: fulfilling it with a synthetic response makes Chromium
  // classify the document's address space differently and cannot validate local-network access.
  await page.route('**/*', (route) => route.request().resourceType() === 'script' ? route.abort() : route.continue());
  await page.goto('http://localhost:1420');
  await page.setContent('<!doctype html><html><body></body></html>');
  const result = await page.evaluate(async (urls) => {
    const images = await Promise.all(urls.map(async (url) => {
      const image = new Image();
      document.body.append(image);
      image.src = url;
      await image.decode();
      const response = await fetch(url, { credentials: 'omit' });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const bytes = new Uint8Array(await response.arrayBuffer());
      return { width: image.naturalWidth, height: image.naturalHeight, bytes: Array.from(bytes) };
    }));
    return { images };
  }, urls);
  assert.equal(result.images.length, 12);
  for (const image of result.images) {
    assert.equal(image.width, 2);
    assert.equal(image.height, 2);
    assert.deepEqual(image.bytes, result.images[0].bytes);
    assert.deepEqual(image.bytes.slice(0, 8), [137, 80, 78, 71, 13, 10, 26, 10]);
  }
  console.log(JSON.stringify({ engine: 'Chromium', imagesDecoded: 12, downloadsByteEqual: 12 }));
} finally {
  await browser.close();
}
