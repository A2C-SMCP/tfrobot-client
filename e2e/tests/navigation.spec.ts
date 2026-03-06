import { test, expect } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

test.beforeEach(async ({ page }) => {
  await setupInvokeMock(page);
  await page.goto('/');
});

test('default page is dashboard', async ({ page }) => {
  await expect(page.getByText('Dashboard').first()).toBeVisible();
});

test('sidebar shows all navigation groups', async ({ page }) => {
  const sidebar = page.locator('.ant-layout-sider');
  await expect(sidebar.getByText('OVERVIEW')).toBeVisible();
  await expect(sidebar.getByText('CONFIGURATION')).toBeVisible();
  await expect(sidebar.getByText('CONNECTION')).toBeVisible();
  await expect(sidebar.getByText('DEVELOPMENT')).toBeVisible();
  await expect(sidebar.getByText('SYSTEM')).toBeVisible();
});

test('sidebar shows all menu items', async ({ page }) => {
  const sidebar = page.locator('.ant-layout-sider');
  await expect(sidebar.getByText('Dashboard')).toBeVisible();
  await expect(sidebar.getByText('MCP Servers')).toBeVisible();
  await expect(sidebar.getByText('Settings')).toBeVisible();
});

test('clicking MCP Servers navigates to MCP page', async ({ page }) => {
  await page.locator('.ant-layout-sider').getByText('MCP Servers').click();
  await expect(page.getByText('Add Server')).toBeVisible();
});

test('clicking Settings navigates to settings page', async ({ page }) => {
  await page.locator('.ant-layout-sider').getByText('Settings').click();
  await expect(page.getByText('Settings').first()).toBeVisible();
});

test('app title is visible in header', async ({ page }) => {
  await expect(page.getByText('TFRobot Client')).toBeVisible();
});
