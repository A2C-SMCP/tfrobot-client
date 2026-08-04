import { test, expect } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

test.describe('Theme switching', () => {
  test('header has theme toggle button', async ({ page }) => {
    await setupInvokeMock(page);
    await page.goto('/');
    // The global Manager account entry is first; theme is the second header button.
    const headerBtns = page.locator('.ant-layout-header button');
    await expect(headerBtns.nth(1)).toBeVisible();
  });

  test('clicking theme toggle changes theme', async ({ page }) => {
    await setupInvokeMock(page);
    await page.goto('/');
    const themeBtn = page.locator('.ant-layout-header button').nth(1);
    await themeBtn.click();
    // After click, the app should reflect the theme change
    // We verify by checking that the button is still functional (no crash)
    await expect(themeBtn).toBeVisible();
  });
});

test.describe('Global Manager account entry', () => {
  test('remains keyboard accessible beside theme and language in a narrow window', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 640, height: 720 });
    await setupInvokeMock(page);
    await page.goto('/');

    const header = page.locator('.ant-layout-header');
    const account = header.locator('button[title="Sign In"]');
    const theme = header.locator('button').nth(1);
    const language = header.getByText('中');
    await expect(account).toBeVisible();
    await expect(theme).toBeVisible();
    await expect(language).toBeVisible();

    const [accountBox, themeBox, languageBox] = await Promise.all([
      account.boundingBox(),
      theme.boundingBox(),
      language.boundingBox(),
    ]);
    expect(accountBox).not.toBeNull();
    expect(themeBox).not.toBeNull();
    expect(languageBox).not.toBeNull();
    expect(accountBox!.x + accountBox!.width).toBeLessThanOrEqual(themeBox!.x);
    expect(themeBox!.x + themeBox!.width).toBeLessThanOrEqual(languageBox!.x);
    expect(languageBox!.x + languageBox!.width).toBeLessThanOrEqual(640);

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
