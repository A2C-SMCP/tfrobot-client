import { test, expect } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

test.describe('MCP Server CRUD', () => {
  test.beforeEach(async ({ page }) => {
    await setupInvokeMock(page);
    await page.goto('/');
    await page.locator('.ant-layout-sider').getByText('Computer').click();
    await page.getByText('Open Details').first().click();
  });

  test('displays default Computer server list with test server', async ({ page }) => {
    await expect(page.getByText('default-stdio-server')).toBeVisible();
    await expect
      .poll(async () => page.evaluate(() => (window as any).__TAURI_INVOKES__))
      .toContainEqual(expect.objectContaining({
        cmd: 'get_mcp_servers',
        args: { instanceId: 'default' },
      }));
  });

  test('shows Add Server button', async ({ page }) => {
    await expect(page.getByText('Add Server')).toBeVisible();
  });

  test('shows Start All and Stop All buttons', async ({ page }) => {
    await expect(page.getByText('Start All')).toBeVisible();
    await expect(page.getByText('Stop All')).toBeVisible();
  });

  test('shows Import and Export buttons', async ({ page }) => {
    await expect(page.getByText('Import Config')).toBeVisible();
    await expect(page.getByText('Export Config')).toBeVisible();
  });

  test('add server opens modal', async ({ page }) => {
    await page.getByText('Add Server').click();
    await expect(page.locator('.ant-modal')).toBeVisible();
  });

  test('opens a second Computer and scopes MCP requests to its instanceId', async ({ page }) => {
    await page.getByText('Back to Computers').click();
    await page.getByText('Second Computer').click();
    await page.getByText('Open Details').nth(1).click();

    await expect(page.getByText('Robot B')).toBeVisible();
    await expect(page.getByText('second-stdio-server')).toBeVisible();
    await expect(page.getByText('default-stdio-server')).not.toBeVisible();
    await expect
      .poll(async () => page.evaluate(() => (window as any).__TAURI_INVOKES__))
      .toContainEqual(expect.objectContaining({
        cmd: 'get_mcp_servers',
        args: { instanceId: 'computer-b' },
      }));
  });
});
