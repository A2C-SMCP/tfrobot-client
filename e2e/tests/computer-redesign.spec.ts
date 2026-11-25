import { expect, test, type Page } from '@playwright/test';
import { mockResponses, setupInvokeMock, shutdownRuntime } from '../fixtures/mock-invoke';

async function openComputer(page: Page) {
  await page.goto('/');
  await page.locator('.ant-layout-sider').getByText('Computer', { exact: true }).click();
  await page.getByRole('button', { name: 'Computer A', exact: true }).click();
}

test('shows one global start action and distinguishes unobserved capabilities from zero', async ({ page }, testInfo) => {
  const instances = mockResponses.list_computer_instances as Array<Record<string, unknown>>;
  await setupInvokeMock(page, {
    list_computer_instances: [{ ...instances[0], running: false, runtime: shutdownRuntime, connected: false, connection: null }],
    enable_computer_runtime_events: [{ instance_id: 'computer-a', snapshot: shutdownRuntime, connection: { present: false, revision: 0, context: null } }],
    get_computer_runtime_snapshots: [{ instance_id: 'computer-a', snapshot: shutdownRuntime, connection: { present: false, revision: 0, context: null } }],
    get_computer_config_adoption: { incarnation: 1, generation: 1, state: 'waiting_start', changes: [], unknown_resources: [] },
  });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await openComputer(page);
  await expect(page.getByRole('button', { name: 'Start', exact: true })).toHaveCount(1);
  await expect(page.getByRole('button', { name: 'Start Runtime', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Start All', exact: true })).toHaveCount(0);
  await expect(page.getByText('Not running · Saved configuration will be used on start', { exact: true })).toBeVisible();
  const capabilities = page.locator('.ant-card').filter({ has: page.getByText('Available in this run', { exact: true }) });
  await expect(capabilities.getByText('—', { exact: true })).toHaveCount(3);
  await expect(page.getByRole('tab', { name: 'Workbench', exact: true })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByText('Advanced Runtime Diagnostics')).toHaveCount(0);
  await page.getByRole('button', { name: 'Language', exact: true }).click();
  await page.screenshot({ path: testInfo.outputPath('workbench-stopped-zh.png'), fullPage: true, animations: 'disabled' });
});

test('switches actual task panels and returns from settings to the selected skills task', async ({ page }, testInfo) => {
  await setupInvokeMock(page);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await openComputer(page);
  await expect(page.getByText('Connection overview', { exact: true })).toBeVisible();
  await expect(page.getByRole('region', { name: 'Skill preview' })).toHaveCount(0);
  await page.getByRole('tab', { name: 'Skills and tools', exact: true }).click();
  await page.getByPlaceholder('Search skills').fill('keyboard');
  await expect(page.getByText('Connection overview', { exact: true })).toBeHidden();
  await page.getByRole('button', { name: 'Open Computer settings' }).click();
  await expect(page.getByRole('textbox', { name: 'Name', exact: true })).toHaveValue('Computer A');
  await expect(page.getByRole('spinbutton', { name: 'MCP startup concurrency' })).toHaveCount(0);
  await expect(page.getByText('Basics', { exact: true })).toBeVisible();
  await expect(page.getByText('Capabilities and access', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Back to runtime workbench' }).click();
  await expect(page.getByRole('tab', { name: 'Skills and tools', exact: true })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByPlaceholder('Search skills')).toHaveValue('keyboard');
  await page.getByRole('tab', { name: 'Workbench', exact: true }).click();
  await page.getByRole('button', { name: 'Language', exact: true }).click();
  await page.screenshot({ path: testInfo.outputPath('workbench-running-zh.png'), fullPage: true, animations: 'disabled' });
  await page.getByRole('button', { name: '打开计算机设置' }).click();
  await page.screenshot({ path: testInfo.outputPath('settings-general-zh.png'), fullPage: true, animations: 'disabled' });
  await page.getByRole('menu', { name: '计算机设置模块' }).getByText('高级', { exact: true }).click();
  await expect(page.getByRole('spinbutton', { name: 'MCP 启动并发数' })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('settings-advanced-zh.png'), fullPage: true, animations: 'disabled' });
});

test('locates the requested user MCP in the workbench without starting it', async ({ page }) => {
  await setupInvokeMock(page);
  await openComputer(page);
  await page.getByRole('button', { name: 'Open Computer settings' }).click();
  await page.getByRole('menu', { name: 'Computer settings sections' }).getByText('MCP Servers', { exact: true }).click();
  const declaration = page.getByRole('row').filter({ hasText: 'computer-a-stdio-server' });
  await declaration.getByRole('button', { name: 'View runtime state', exact: true }).click();
  const resource = page.locator('[data-mcp-bundle="computer-a-stdio-server"]');
  await expect(resource).toHaveAttribute('aria-current', 'true');
  await expect(resource).toBeFocused();
  await page.getByRole('button', { name: 'Open Computer settings' }).click();
  await page.getByRole('menu', { name: 'Computer settings sections' }).getByText('MCP Servers', { exact: true }).click();
  await declaration.getByRole('button', { name: 'View runtime state', exact: true }).click();
  await expect(resource).toBeFocused();
  const commands = await page.evaluate(() => (window as typeof window & { __TAURI_INVOKES__?: Array<{ cmd: string }> }).__TAURI_INVOKES__ ?? []);
  expect(commands.some(({ cmd }) => ['start_mcp_server', 'restart_computer_instance'].includes(cmd))).toBe(false);
});

test('keeps authorization edits separate from the saved policy and diagnostics off the main task', async ({ page }, testInfo) => {
  await setupInvokeMock(page);
  await openComputer(page);
  await page.getByRole('tab', { name: 'Diagnostics', exact: true }).click();
  await expect(page.getByRole('tab', { name: 'Logs', exact: true })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByRole('heading', { name: 'Logs', exact: true })).toBeVisible();
  await expect(page.getByRole('tab', { name: 'Runtime evidence', exact: true })).toHaveCount(0);
  await expect(page.getByRole('heading', { name: 'MCP Runtime', exact: true })).toBeHidden();
  await page.getByRole('button', { name: 'Open Computer settings' }).click();
  await page.getByRole('menu', { name: 'Computer settings sections' }).getByText('Built-in Tools', { exact: true }).click();
  await expect(page.getByRole('switch', { name: 'Enable Robot control' })).toHaveCount(0);
  await page.getByRole('button', { name: 'Edit authorization', exact: true }).click();
  const editor = page.getByRole('dialog', { name: 'Edit authorization' });
  await editor.getByRole('switch', { name: 'Enable Robot control' }).click();
  await editor.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(editor).toBeHidden();
  await expect(page.getByText('Disabled in the saved policy', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Language', exact: true }).click();
  await page.screenshot({ path: testInfo.outputPath('settings-builtins-zh.png'), fullPage: true, animations: 'disabled' });
  const commands = await page.evaluate(() => (window as typeof window & { __TAURI_INVOKES__?: Array<{ cmd: string }> }).__TAURI_INVOKES__ ?? []);
  expect(commands.some(({ cmd }) => cmd === 'update_remote_control_policy')).toBe(false);
});
