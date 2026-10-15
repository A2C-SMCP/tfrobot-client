import { expect, test } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

test('keeps Computer information and operations available in both collection layouts', async ({ page }) => {
  await setupInvokeMock(page);
  await page.goto('/');
  const menu = page.locator('.ant-layout-sider');
  await menu.getByText('Computer', { exact: true }).click();
  const card = page.locator('.ant-card').filter({ has: page.getByRole('button', { name: 'Computer A', exact: true }) });
  const assertInformation = async () => {
    await expect(card.getByText('computer-a', { exact: true })).toBeVisible();
    for (const action of ['Stop', 'Connect', 'Edit', 'Duplicate', 'Delete']) {
      await expect(card.getByRole('button', { name: action, exact: true })).toBeVisible();
    }
  };
  await assertInformation();
  await page.getByText('Icons', { exact: true }).click();
  await assertInformation();
  await menu.getByText('Settings', { exact: true }).click();
  await menu.getByText('Computer', { exact: true }).click();
  await expect(page.getByRole('radio', { name: /Icons/ })).toBeChecked();
  await card.getByRole('button', { name: 'Computer A', exact: true }).click();
  await expect(page.getByRole('navigation', { name: 'Workbench sections' })).toBeVisible();
  await page.getByRole('navigation', { name: 'Workbench sections' }).getByRole('button', { name: 'Skills', exact: true }).click();
  await expect(page.getByRole('region', { name: 'Skill preview' })).toBeInViewport();
});

for (const width of [1280, 375]) {
  test(`keeps the collection, settings form and modal usable at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await setupInvokeMock(page);
    await page.goto('/');
    await page.locator('.ant-layout-sider').getByText('Computer', { exact: true }).click();
    await expect(page.getByRole('button', { name: 'Computer A', exact: true })).toBeVisible();
    await page.getByRole('button', { name: 'Create Computer', exact: true }).click();
    await expect(page.getByRole('dialog', { name: 'Create Computer' })).toBeVisible();
    await expect(page.getByRole('dialog').getByRole('textbox', { name: 'Name', exact: true })).toBeVisible();
    await page.getByRole('dialog').getByRole('button', { name: 'Cancel' }).click();
    await page.getByRole('button', { name: 'Computer A', exact: true }).click();
    await page.getByRole('button', { name: 'Open Computer settings' }).click();
    await expect(page.getByRole('textbox', { name: 'Name', exact: true })).toHaveValue('Computer A');
    await expect(page.getByRole('spinbutton', { name: 'MCP startup concurrency' })).toHaveValue('5');
    await expect(page.getByRole('button', { name: /Save$/ })).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
  });
}
