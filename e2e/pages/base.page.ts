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
    // The account entry is first; theme remains beside it and before language.
    return this.header.locator('button').nth(1);
  }

  async getLanguageToggle() {
    // The EN/中 button in header
    return this.header.locator('button').nth(2);
  }
}
