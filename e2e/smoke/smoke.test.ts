// Smoke tests: verify the real Tauri app starts and core paths work.
// These run against a built release binary via Tauri Driver + WebDriver.
// NOT meant to replace Playwright E2E — only covers critical paths.

describe('TFRobot Smoke Test', () => {

  // ── App Startup ──

  it('app window opens with a title', async () => {
    const title = await browser.getTitle();
    expect(title).toBeTruthy();
  });

  it('window size meets minimum (≥800x600)', async () => {
    const { width, height } = await browser.getWindowRect();
    expect(width).toBeGreaterThanOrEqual(800);
    expect(height).toBeGreaterThanOrEqual(600);
  });

  // ── Dashboard ──

  it('Dashboard page renders', async () => {
    const dashboard = await $('[data-testid="dashboard"]');
    await dashboard.waitForDisplayed({ timeout: 10000 });
  });

  it('Dashboard shows runtime detection results', async () => {
    const runtimeSection = await $('[data-testid="runtimes"]');
    await runtimeSection.waitForDisplayed();
    const text = await runtimeSection.getText();
    expect(text).toMatch(/node|python|uv|pnpm/i);
  });

  // ── Navigation ──

  it('sidebar navigation works', async () => {
    const settingsMenu = await $('li*=Settings');
    await settingsMenu.click();

    const settingsContent = await $('[data-testid="settings"]');
    await settingsContent.waitForDisplayed({ timeout: 5000 });
  });

  // ── MCP Servers ──

  it('MCP page loads', async () => {
    const mcpMenu = await $('li*=MCP');
    await mcpMenu.click();

    const mcpContent = await $('[data-testid="mcp-config"]');
    await mcpContent.waitForDisplayed({ timeout: 5000 });
  });
});
