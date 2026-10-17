import { expect, test } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

test('development offers fixed environments and remembers selection across launches', async ({ page }) => {
  await setupInvokeMock(page);
  await page.goto('/');
  await page.getByRole('button', { name: /Sign In/ }).click();
  const dialog = page.getByRole('dialog', { name: 'Manager Account' });
  await expect(dialog.getByText('Test', { exact: true })).toBeVisible();
  await dialog.getByText('Test', { exact: true }).click();
  const options = page.locator('.ant-select-item-option');
  await expect(options.filter({ hasText: /^Test$/ })).toBeVisible();
  await expect(options.filter({ hasText: /^Production \(including Beta\)$/ })).toBeVisible();
  await options.filter({ hasText: /^Staging$/ }).click();
  await page.reload();
  await page.getByRole('button', { name: /Sign In/ }).click();
  await expect(dialog.getByText('Staging', { exact: true })).toBeVisible();
  await dialog.getByLabel('Phone or email').fill('client@example.com');
  await dialog.getByLabel('Password', { exact: true }).fill('fixture-password');
  await dialog.getByRole('button', { name: /Sign In/ }).click();
  await expect.poll(() => page.evaluate(() => {
    const calls = (window as unknown as { __TAURI_INVOKES__: { cmd: string; args?: unknown }[] }).__TAURI_INVOKES__;
    return calls.find((call) => call.cmd === 'manager_login')?.args;
  })).toEqual({ environment: 'staging', identifier: 'client@example.com', password: 'fixture-password' });
});

test('packaged UI ignores saved test choice and sends only prod', async ({ page }) => {
  await setupInvokeMock(page, {
    get_manager_environment_policy: {
      buildMode: 'production', allowedEnvironments: ['prod'], defaultEnvironment: 'prod',
    },
  });
  await page.addInitScript(() => localStorage.setItem('tfrobot-client.development.managerEnvironment', 'test'));
  await page.goto('/');
  await page.getByRole('button', { name: /Sign In/ }).click();
  const dialog = page.getByRole('dialog', { name: 'Manager Account' });
  await expect(dialog.getByText('Production (including Beta)', { exact: true })).toBeVisible();
  await expect(dialog.getByRole('combobox')).toHaveCount(0);
  await dialog.getByLabel('Phone or email').fill('client@example.com');
  await dialog.getByLabel('Password', { exact: true }).fill('fixture-password');
  await dialog.getByRole('button', { name: /Sign In/ }).click();
  await expect.poll(() => page.evaluate(() => {
    const calls = (window as unknown as { __TAURI_INVOKES__: { cmd: string; args?: { environment: string } }[] }).__TAURI_INVOKES__;
    return calls.find((call) => call.cmd === 'manager_login')?.args?.environment;
  })).toBe('prod');
});
