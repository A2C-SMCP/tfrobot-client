import { Page, Locator } from '@playwright/test';

export class BasePage {
  readonly page: Page;
  readonly sidebar: Locator;
  readonly header: Locator;

  constructor(page: Page) {
    this.page = page;
    this.sidebar = page.locator('.ant-layout-sider');
    this.header = page.locator('.ant-layout-header');
  }

  async navigateTo(menuText: string) {
    await this.sidebar.getByText(menuText, { exact: false }).click();
  }

  async getThemeToggle() {
    // The sun/moon icon button in header
    return this.header.locator('button').first();
  }

  async getLanguageToggle() {
    // The EN/中 button in header
    return this.header.locator('button').nth(1);
  }
}
