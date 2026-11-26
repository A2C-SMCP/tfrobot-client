import { expect, test, type Page } from '@playwright/test';
import { mockResponses, setupInvokeMock, shutdownRuntime, startedRuntime } from '../fixtures/mock-invoke';

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

test('filters tools by MCP and opens the exact call in the computer log', async ({ page }) => {
  const tool = { description: 'Preview document content', inputSchema: { type: 'object', properties: {} } };
  const correlationId = 'request-preview-outline';
  const timestamp = '2026-11-26T06:13:05Z';
  await setupInvokeMock(page, {
    get_available_tools: [
      { ...tool, name: 'preview_outline', displayName: 'Preview outline', server: 'doc-assistant', bundleId: 'documents' },
      { ...tool, name: 'list_windows', displayName: 'List windows', server: 'desktop', bundleId: 'desktop-tools' },
    ],
    execute_tool: { success: false, error: 'Document is locked', duration_ms: 21, correlation_id: correlationId },
    get_activity: { items: [{ id: 7, timestamp, scope: { kind: 'computer', computer_id: 'computer-a' },
      level: 'error', category: 'tool', event_type: 'tool_call', operation: 'preview_outline',
      outcome: 'failed', message: 'Tool preview_outline failed', fields: { server_name: 'doc-assistant' },
      correlation_id: correlationId }], total: 1, limit: 50, offset: 0 },
    get_tool_history: [{ timestamp, req_id: correlationId, computer_instance_id: 'computer-a',
      server: 'doc-assistant', tool: 'preview_outline', success: false, error: 'Document is locked',
      parameters: { topic: 'Annual report' } }],
  });
  await openComputer(page);
  await expect(page.getByRole('tab')).toHaveText(['Workbench', 'Skills and tools', 'Desktop Resources', 'Logs']);
  await page.getByRole('tab', { name: 'Logs', exact: true }).click();
  const logSearch = page.getByPlaceholder('Search logs…');
  await logSearch.fill('previous query');
  await logSearch.press('Enter');
  await page.getByRole('tab', { name: 'Skills and tools' }).click();
  await page.getByRole('radiogroup').getByText('Tools', { exact: true }).click();
  const preview = page.getByRole('button', { name: 'Preview outline doc-assistant' });
  const windows = page.getByRole('button', { name: 'List windows desktop' });
  await expect(preview).toBeVisible();
  await expect(windows).toBeVisible();
  await page.getByRole('combobox', { name: 'Filter by MCP server' }).click();
  await page.locator('.ant-select-item-option-content').getByText('desktop', { exact: true }).click();
  await page.keyboard.press('Escape');
  await expect(windows).toHaveCount(0);
  await page.getByPlaceholder('Search tools').fill('missing');
  await expect(preview).toHaveCount(0);
  await page.getByPlaceholder('Search tools').fill('preview');
  await expect(preview).toHaveAttribute('aria-pressed', 'true');
  await page.getByRole('button', { name: 'Execute', exact: true }).click();
  await page.getByRole('button', { name: 'View in logs' }).click();
  await expect(page.getByRole('tab', { name: 'Logs', exact: true })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByRole('row').filter({ hasText: 'Tool preview_outline failed' })).toBeFocused();
  await expect(page.getByRole('tabpanel', { name: 'Logs' }).getByText('Document is locked', { exact: true })).toBeVisible();
  await expect(page.getByText(/Annual report/)).toBeVisible();
  const commands = await page.evaluate(() => (window as typeof window & {
    __TAURI_INVOKES__?: Array<{ cmd: string; args: unknown }>;
  }).__TAURI_INVOKES__ ?? []);
  expect(commands).toContainEqual({ cmd: 'get_tool_history', args: { instanceId: 'computer-a', requestId: correlationId } });
  expect(commands).toContainEqual(expect.objectContaining({ cmd: 'get_activity', args: { query: expect.objectContaining({
    scope: { kind: 'computer', computer_id: 'computer-a' }, correlation_id: correlationId,
  }) } }));
  await page.getByRole('button', { name: 'Back to logs' }).click();
  await expect(page.getByRole('button', { name: 'Back to logs' })).toHaveCount(0);
  await expect(logSearch).toHaveValue('previous query');
});

test('combines desktop search and MCP filters and opens a full window preview by keyboard', async ({ page }) => {
  const [computer] = mockResponses.list_computer_instances as Array<Record<string, unknown>>;
  const runtime = { ...startedRuntime, active_mcp_servers: 2, mcp_servers: 2 };
  const snapshot = { instance_id: 'computer-a', snapshot: runtime, connection: { present: false, revision: 0, context: null } };
  const editor = { bundleId: 'editor', title: 'Annual report draft', uri: 'window://editor/report', server: 'Editor MCP' };
  const browser = { bundleId: 'browser', title: 'Annual report portal', uri: 'window://browser/report', server: 'Browser MCP' };
  const text = `${'A long report. '.repeat(70)}End of report`;
  await setupInvokeMock(page, {
    list_computer_instances: [{ ...computer, runtime }],
    enable_computer_runtime_events: [snapshot],
    get_computer_runtime_snapshots: [snapshot],
    get_desktop: { status: 'complete', windows: [editor, browser] },
    get_window_detail: { ...editor, contents: [{ type: 'text', uri: editor.uri, text }] },
  });
  await openComputer(page);
  await page.getByRole('tab', { name: 'Desktop Resources' }).click();
  const draft = page.getByRole('button', { name: 'Annual report draft' });
  const portal = page.getByRole('button', { name: 'Annual report portal' });
  await expect(draft).toBeVisible();
  await expect(portal).toBeVisible();
  await page.getByRole('combobox', { name: 'Filter by MCP server' }).click();
  await page.locator('.ant-select-item-option-content').getByText('Browser MCP', { exact: true }).click();
  await page.keyboard.press('Escape');
  await expect(portal).toHaveCount(0);
  await page.getByPlaceholder('Search desktop windows').fill('missing');
  await expect(draft).toHaveCount(0);
  await page.getByPlaceholder('Search desktop windows').fill('draft');
  await draft.press('Enter');
  const dialog = page.getByRole('dialog', { name: 'Annual report draft' });
  await expect(dialog.getByText(text)).toBeVisible();
  await expect(dialog.locator(':focus')).toHaveCount(1);
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
  await expect(draft).toBeFocused();
});

test('keeps authorization edits separate from the saved policy and diagnostics off the main task', async ({ page }, testInfo) => {
  await setupInvokeMock(page);
  await openComputer(page);
  await page.getByRole('tab', { name: 'Logs', exact: true }).click();
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
