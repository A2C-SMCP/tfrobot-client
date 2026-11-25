import { expect, test, type Page } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

async function openEnvironmentSettings(page: Page) {
  await page.getByRole('menuitem', { name: 'Settings' }).click();
  await page.getByRole('tab', { name: 'Sign-in environment' }).click();
  return page.getByRole('tabpanel', { name: 'Sign-in environment' });
}

async function chooseEnvironment(page: Page, name: string) {
  await page.getByRole('combobox', { name: 'Sign-in environment' }).press('ArrowDown');
  await page.locator('.ant-select-item-option').filter({ hasText: name }).click();
}

function observedInvokes(page: Page, command: string) {
  return page.evaluate((cmd) => {
    const calls = (window as unknown as { __TAURI_INVOKES__: { cmd: string; args?: unknown }[] }).__TAURI_INVOKES__;
    return calls.filter((call) => call.cmd === cmd).map((call) => call.args);
  }, command);
}

test('uses the saved sign-in environment from Settings after reloading', async ({ page }) => {
  await setupInvokeMock(page, {
    manager_login: { kind: 'authenticated', user: { userId: '9', accountId: '16', accountName: 'fixture' } },
  });
  // The retired development-only browser preference must not select the server on upgrade.
  await page.addInitScript(() => localStorage.setItem('tfrobot-client.development.managerEnvironment', 'test'));
  await page.goto('/');

  const settings = await openEnvironmentSettings(page);
  await expect(settings.getByText('Production (including Beta)', { exact: true })).toBeVisible();
  await chooseEnvironment(page, 'Staging');
  await expect(settings.locator('.ant-select-selection-item')).toHaveText('Staging');
  expect(await observedInvokes(page, 'set_manager_login_environment')).toEqual([
    { environment: 'staging', confirmedRevision: null },
  ]);

  await page.reload();
  await page.getByRole('button', { name: /Sign In/i }).click();
  const dialog = page.getByRole('dialog', { name: 'Manager Account' });
  await expect(dialog.getByText('Staging', { exact: true })).toBeVisible();
  await expect(dialog.getByRole('combobox')).toHaveCount(0);
  await dialog.getByLabel('Phone or email').fill('client@example.com');
  await dialog.getByLabel('Password', { exact: true }).fill('fixture-password');
  await dialog.getByRole('button', { name: /Sign In/i }).click();
  await expect.poll(() => observedInvokes(page, 'manager_login')).toEqual([
    { expectedEnvironment: 'staging', identifier: 'client@example.com', password: 'fixture-password' },
  ]);
});

test('confirms a signed-in environment switch and opens the new sign-in form', async ({ page }) => {
  await setupInvokeMock(page, {
    manager_get_context: {
      revision: 8, authState: 'authenticated', environment: 'prod',
      contextKey: { environment: 'prod', accountId: '16', organizationId: '42' },
      user: { id: '9', nickname: 'Fixture', email: '', phone: '' },
      account: { id: '16', name: 'Fixture Account', nickname: '', avatar: '', employeeNo: '' },
      organization: { id: '42', name: 'Fixture Organization', organizationType: 'enterprise' },
      permissions: [],
    },
  });
  await page.goto('/');
  await expect(page.getByRole('button', { name: 'Fixture Organization, Fixture Account' })).toBeVisible();
  const settings = await openEnvironmentSettings(page);
  await expect(settings.getByText('Production (including Beta)', { exact: true })).toBeVisible();

  const confirmation = page.getByRole('dialog', { name: 'Change the sign-in environment?' });
  await chooseEnvironment(page, 'Test');
  await confirmation.getByRole('button', { name: 'Cancel' }).click();
  await expect(confirmation).toBeHidden();
  expect(await observedInvokes(page, 'set_manager_login_environment')).toEqual([]);

  await chooseEnvironment(page, 'Test');
  await confirmation.getByRole('button', { name: 'Switch and sign out' }).click();
  const signIn = page.getByRole('dialog', { name: 'Manager Account' });
  await expect(signIn.getByText('Test', { exact: true })).toBeVisible();
  expect(await observedInvokes(page, 'set_manager_login_environment')).toEqual([
    { environment: 'test', confirmedRevision: 8 },
  ]);
});
