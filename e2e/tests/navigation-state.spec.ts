import { test, expect, type Page } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

const menu = (page: Page, name: string) => page.locator('.ant-layout-sider').getByText(name, { exact: true });

test.beforeEach(async ({ page }) => {
  await setupInvokeMock(page, { detect_runtimes: [], get_detected_path: '/usr/bin', get_activity: { items: [], total: 0, limit: 50, offset: 0 } });
  await page.goto('/');
  await expect(page.getByTestId('chat')).toBeVisible();
});

test('initializes lazily and restores Settings tab without fetching settings again', async ({ page }) => {
  const calls = () => page.evaluate(() => (window as unknown as {
    __TAURI_INVOKES__: { cmd: string }[];
  }).__TAURI_INVOKES__.map(({ cmd }) => cmd));
  expect(await calls()).not.toContain('get_activity');
  expect(await calls()).not.toContain('detect_runtimes');
  await menu(page, 'Settings').click();
  await page.getByRole('tab', { name: 'Runtime', exact: true }).click();
  const settingsCalls = (await calls()).filter((cmd) => cmd === 'get_settings').length;
  await menu(page, 'Computer').click();
  await menu(page, 'Settings').click();
  await expect(page.getByRole('tab', { name: 'Runtime', exact: true })).toHaveAttribute('aria-selected', 'true');
  expect((await calls()).filter((cmd) => cmd === 'get_settings').length).toBe(settingsCalls);
});

test('restores Computer settings, tab drafts and the selected instance after menu round trips', async ({ page }) => {
  await menu(page, 'Computer').click();
  await page.getByRole('button', { name: 'Second Computer', exact: true }).click();
  await page.getByRole('button', { name: 'Open Computer settings', exact: true }).click();
  const name = page.getByRole('textbox', { name: 'Name', exact: true });
  await name.fill('unfinished name');
  await page.getByRole('menu', { name: 'Computer settings sections' }).getByText('Skills', { exact: true }).click();
  await menu(page, 'Settings').click();
  await menu(page, 'Computer').click();
  await expect(page.getByRole('heading', { name: 'Second Computer Settings' })).toBeVisible();
  await page.getByRole('menu', { name: 'Computer settings sections' }).getByText('General', { exact: true }).click();
  await expect(name).toHaveValue('unfinished name');
  const saves = await page.evaluate(() => (window as unknown as {
    __TAURI_INVOKES__: { cmd: string }[];
  }).__TAURI_INVOKES__.filter(({ cmd }) => cmd === 'rename_computer_instance').length);
  expect(saves).toBe(0);
});

test('restores an unsubmitted Activity search after visiting other menus', async ({ page }) => {
  await menu(page, 'Activity').click();
  const search = page.getByPlaceholder('Search keyword');
  await search.fill('unfinished query');
  await menu(page, 'Computer').click();
  await menu(page, 'Activity').click();
  await expect(search).toHaveValue('unfinished query');
});

test('restores the window scroll of each Settings tab', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 500 });
  await menu(page, 'Settings').click();
  await page.getByRole('tab', { name: 'Runtime', exact: true }).click();
  await page.getByText('Custom Paths', { exact: true }).click();
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollHeight)).toBeGreaterThan(700);
  await page.evaluate(() => window.scrollTo(0, 160));
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(160);
  // DOM clicks avoid Playwright scrolling the old tab into view before switching.
  await page.getByRole('tab', { name: 'About', exact: true }).evaluate((node: HTMLElement) => node.click());
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);
  await page.getByRole('tab', { name: 'Runtime', exact: true }).evaluate((node: HTMLElement) => node.click());
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(160);
});

test('does not inherit another Computer settings scroll on a first visit', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 500 });
  await menu(page, 'Computer').click();
  await page.getByRole('button', { name: 'Computer A', exact: true }).click();
  await page.getByRole('button', { name: 'Open Computer settings', exact: true }).click();
  await page.evaluate(() => window.scrollTo(0, 300));
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(300);
  for (const name of ['Back to Computer', 'Back to Computers', 'Second Computer', 'Open Computer settings']) {
    await page.getByRole('button', { name, exact: true }).evaluate((node: HTMLElement) => node.click());
  }
  await expect(page.getByRole('heading', { name: 'Second Computer Settings' })).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(0);
});
