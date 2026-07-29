import { test, expect } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

test.describe('MCP Server configuration', () => {
  test.beforeEach(async ({ page }) => {
    await setupInvokeMock(page);
    await page.goto('/');
    await page.locator('.ant-layout-sider').getByText('Computer').click();
    await page.getByRole('button', { name: 'Computer A', exact: true }).click();
    await page.getByRole('button', { name: 'Open Computer settings' }).click();
    await page
      .getByRole('menu', { name: 'Computer settings sections' })
      .getByText('MCP Servers', { exact: true })
      .click();
  });

  test('displays first Computer server list with test server', async ({ page }) => {
    await expect(page.getByRole('row', { name: /computer-a-stdio-server/ })).toBeVisible();
    await expect
      .poll(async () => page.evaluate(() => (window as any).__TAURI_INVOKES__))
      .toContainEqual(expect.objectContaining({
        cmd: 'get_computer_config_state',
        args: { instanceId: 'computer-a' },
      }));
  });

  test('shows Add Server button', async ({ page }) => {
    await expect(page.getByText('Add Server')).toBeVisible();
  });

  test('shows Start All and Stop All buttons', async ({ page }) => {
    await page.getByRole('button', { name: 'Back to Computer' }).click();
    const runtimeTab = page.getByRole('tab', { name: /Runtime/ });
    if (await runtimeTab.isVisible()) {
      await runtimeTab.click();
    } else {
      await page.locator('.ant-tabs-nav-more').click();
      await page.getByText('Runtime', { exact: true }).last().click();
    }
    await expect(page.getByText('Start All')).toBeVisible();
    await expect(page.getByText('Stop All')).toBeVisible();
  });

  test('shows Import and Export buttons', async ({ page }) => {
    await expect(page.getByText('Import Config')).toBeVisible();
    await expect(page.getByText('Export Config')).toBeVisible();
  });

  test('add server opens modal', async ({ page }) => {
    await page.getByText('Add Server').click();
    await expect(page.getByRole('dialog', { name: 'Add Server' })).toBeVisible();
  });

  test('opens a second Computer and scopes MCP requests to its instanceId', async ({ page }) => {
    await page.locator('.ant-layout-sider').getByText('Computer').click();
    await page.getByRole('button', { name: 'Second Computer', exact: true }).click();
    await page.getByRole('button', { name: 'Open Computer settings' }).click();
    await page
      .getByRole('menu', { name: 'Computer settings sections' })
      .getByText('MCP Servers', { exact: true })
      .click();

    await expect(page.getByRole('row', { name: /second-stdio-server/ })).toBeVisible();
    await expect(page.getByRole('row', { name: /computer-a-stdio-server/ })).not.toBeVisible();
    await expect
      .poll(async () => page.evaluate(() => (window as any).__TAURI_INVOKES__))
      .toContainEqual(expect.objectContaining({
        cmd: 'get_computer_config_state',
        args: { instanceId: 'computer-b' },
      }));
  });
});
