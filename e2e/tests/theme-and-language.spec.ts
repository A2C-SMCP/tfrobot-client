import { test, expect } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

test.describe('Theme switching', () => {
  test('applies and switches the resolved theme to the application surface', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'light' });
    await setupInvokeMock(page);
    await page.goto('/');
    const app = page.locator('.client-app');
    await expect(app).toHaveCSS('color-scheme', 'light');
    await page.getByRole('button', { name: 'Dark', exact: true }).click();
    await expect(app).toHaveCSS('color-scheme', 'dark');
    await expect(app).toHaveCSS('background-color', 'rgb(28, 28, 30)');
    await page.getByRole('button', { name: 'Light', exact: true }).click();
    await expect(app).toHaveCSS('color-scheme', 'light');
    await expect(app).toHaveCSS('background-color', 'rgb(250, 250, 250)');
  });
});

test.describe('Global Manager account entry', () => {
  test('remains keyboard accessible in the sidebar at a narrow width', async ({ page }) => {
    await page.setViewportSize({ width: 640, height: 720 });
    await page.emulateMedia({ colorScheme: 'light' });
    await setupInvokeMock(page);
    await page.goto('/');
    const account = page.locator('.ant-layout-sider button[aria-haspopup="dialog"]');
    await expect(account).toBeVisible();
    await expect(page.getByRole('button', { name: 'Dark', exact: true })).toBeVisible();
    await expect(page.locator('.ant-layout-header').getByText('中')).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(640);
    await account.focus();
    await account.press('Enter');
    const dialog = page.getByRole('dialog', { name: 'Manager Account' });
    await expect(dialog).toBeVisible();
    await dialog.press('Escape');
    await expect(dialog).toBeHidden();
    await expect(account).toBeFocused();
  });
});

test.describe('Language switching', () => {
  test('header has language toggle button', async ({ page }) => {
    await setupInvokeMock(page);
    await page.goto('/');
    await expect(page.locator('.ant-layout-header').getByText('中')).toBeVisible();
  });

  test('clicking language toggle switches to Chinese', async ({ page }) => {
    await setupInvokeMock(page);
    await page.goto('/');
    await page.locator('.ant-layout-header').getByText('中').click();
    await expect(page.locator('.ant-layout-header').getByText('EN')).toBeVisible();
  });
});
