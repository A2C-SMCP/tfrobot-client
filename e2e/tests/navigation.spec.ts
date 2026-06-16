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
  await expect(sidebar.getByText('Skills')).toBeVisible();
  await expect(sidebar.getByText('Settings')).toBeVisible();
});

test('Skills belongs to configuration group', async ({ page }) => {
  const sidebarText = (await page.locator('.ant-layout-sider').textContent()) ?? '';

  expect(sidebarText.indexOf('CONFIGURATION')).toBeLessThan(sidebarText.indexOf('Skills'));
  expect(sidebarText.indexOf('Skills')).toBeLessThan(sidebarText.indexOf('CONNECTION'));
});

test('clicking MCP Servers navigates to MCP page', async ({ page }) => {
  await page.locator('.ant-layout-sider').getByText('MCP Servers').click();
  await expect(page.getByText('Add Server')).toBeVisible();
});

test('clicking Settings navigates to settings page', async ({ page }) => {
  await page.locator('.ant-layout-sider').getByText('Settings').click();
  await expect(page.getByText('Settings').first()).toBeVisible();
});

test('clicking Skills navigates to skills page', async ({ page }) => {
  await page.locator('.ant-layout-sider').getByText('Skills').click();
  await expect(page.getByText('Refresh')).toBeVisible();
  await expect(page.getByText('Open Root Folder')).toBeVisible();
  await expect(page.getByText('Runs local tasks')).toBeVisible();
  await expect(page.getByText('Local', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: /Preview/ }).click();
  await expect(page.getByRole('heading', { name: 'Demo Skill' })).toBeVisible();
  await expect(page.getByText('Uses')).toBeVisible();
  await expect(page.getByText('Enable', { exact: true })).toHaveCount(0);
  await expect(page.getByText('Disable', { exact: true })).toHaveCount(0);
});

test('app title is visible in header', async ({ page }) => {
  await expect(page.getByText('TFRobot Client')).toBeVisible();
});
