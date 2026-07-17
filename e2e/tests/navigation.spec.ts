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
  await expect(sidebar.getByText('CONNECTION LAYER')).toBeVisible();
  await expect(sidebar.getByText('DEVELOPMENT')).toBeVisible();
  await expect(sidebar.getByText('SYSTEM')).toBeVisible();
});

test('sidebar shows all menu items', async ({ page }) => {
  const sidebar = page.locator('.ant-layout-sider');
  await expect(sidebar.getByText('Dashboard')).toBeVisible();
  await expect(sidebar.getByText('Computer')).toBeVisible();
  await expect(sidebar.getByText('Robot Connections')).toBeVisible();
  await expect(sidebar.getByText('Settings')).toBeVisible();
});

test('clicking Computer navigates to Computer page', async ({ page }) => {
  await page.locator('.ant-layout-sider').getByText('Computer').click();
  await expect(page.getByRole('button', { name: 'Computer A', exact: true })).toBeVisible();
});

test('clicking Settings navigates to settings page', async ({ page }) => {
  await page.locator('.ant-layout-sider').getByText('Settings').click();
  await expect(page.getByText('Settings').first()).toBeVisible();
});

test('app title is visible in header', async ({ page }) => {
  await expect(page.getByText('TFRobot Client')).toBeVisible();
});
