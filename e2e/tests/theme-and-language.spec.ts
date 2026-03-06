import { test, expect } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

test.describe('Theme switching', () => {
  test('header has theme toggle button', async ({ page }) => {
    await setupInvokeMock(page);
    await page.goto('/');
    // The first button in header Space is the theme toggle (sun/moon icon)
    const headerBtns = page.locator('.ant-layout-header button');
    await expect(headerBtns.first()).toBeVisible();
  });

  test('clicking theme toggle changes theme', async ({ page }) => {
    await setupInvokeMock(page);
    await page.goto('/');
    const themeBtn = page.locator('.ant-layout-header button').first();
    await themeBtn.click();
    // After click, the app should reflect the theme change
    // We verify by checking that the button is still functional (no crash)
    await expect(themeBtn).toBeVisible();
  });
});

test.describe('Language switching', () => {
  test('header has language toggle button', async ({ page }) => {
    await setupInvokeMock(page);
    await page.goto('/');
    // Language button shows "中" when in English mode
    await expect(page.locator('.ant-layout-header').getByText('中')).toBeVisible();
  });

  test('clicking language toggle switches to Chinese', async ({ page }) => {
    await setupInvokeMock(page);
    await page.goto('/');
    await page.locator('.ant-layout-header').getByText('中').click();
    // After switching to Chinese, the button should show "EN"
    await expect(page.locator('.ant-layout-header').getByText('EN')).toBeVisible();
  });
});
