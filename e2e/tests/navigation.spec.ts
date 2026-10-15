import { test, expect } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

test.beforeEach(async ({ page }) => {
  await setupInvokeMock(page);
  await page.goto('/');
});

test('default page is Chat', async ({ page }) => {
  await expect(page.getByTestId('chat')).toBeVisible();
  await expect(page.getByText('Sign in to Manager to chat with an available Robot.')).toBeVisible();
});

test('sidebar has no group captions and keeps Settings above the bottom account entry', async ({ page }) => {
  const sidebar = page.locator('.ant-layout-sider');
  for (const caption of ['OVERVIEW', 'CONNECTION LAYER', 'DEVELOPMENT', 'SYSTEM']) {
    await expect(sidebar.getByText(caption, { exact: true })).toHaveCount(0);
  }
  const activity = await sidebar.getByText('Activity', { exact: true }).boundingBox();
  const settings = await sidebar.getByText('Settings', { exact: true }).boundingBox();
  const account = await sidebar.locator('button[aria-haspopup="dialog"]').boundingBox();
  expect(settings!.y).toBeGreaterThan(activity!.y + activity!.height);
  expect(settings!.y + settings!.height).toBeLessThanOrEqual(account!.y);
  expect(account!.y + account!.height).toBeGreaterThan(page.viewportSize()!.height - 40);
});

test('sidebar shows all menu items', async ({ page }) => {
  const sidebar = page.locator('.ant-layout-sider');
  await expect(sidebar.getByText('Dashboard')).toHaveCount(0);
  await expect(sidebar.getByText('Chat')).toBeVisible();
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
