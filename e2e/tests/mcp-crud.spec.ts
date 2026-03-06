import { test, expect } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

test.describe('MCP Server CRUD', () => {
  test.beforeEach(async ({ page }) => {
    await setupInvokeMock(page);
    await page.goto('/');
    await page.locator('.ant-layout-sider').getByText('MCP Servers').click();
  });

  test('displays server list with test server', async ({ page }) => {
    await expect(page.getByText('test-stdio-server')).toBeVisible();
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
});
