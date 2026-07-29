import { expect, test, type Page } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

async function openComputerTab(page: Page, label: string) {
  const visibleTab = page.getByRole('tab').filter({ hasText: label });
  if (await visibleTab.isVisible()) {
    await visibleTab.click();
    return;
  }

  await page.locator('.ant-tabs-nav-more').click();
  await page
    .locator('.ant-tabs-dropdown:not(.ant-tabs-dropdown-hidden)')
    .getByText(label, { exact: true })
    .click();
}

test.describe('Computer configuration, runtime, and diagnostics boundaries', () => {
  test.beforeEach(async ({ page }) => {
    await setupInvokeMock(page);
    await page.goto('/');
    await page.locator('.ant-layout-sider').getByText('Computer').click();
    await page.getByRole('button', { name: 'Computer A', exact: true }).click();
  });

  test('keeps profile, config, runtime, and diagnostics operations semantically separate', async ({ page }) => {
    const profileGroup = page
      .getByText('Profile & Configuration', { exact: true })
      .locator('..')
      .locator('..');
    await expect(profileGroup.getByRole('button', { name: 'Edit' })).toBeVisible();
    await expect(profileGroup.getByRole('button', { name: 'Duplicate' })).toBeVisible();
    await expect(profileGroup.getByRole('button', { name: 'Delete' })).toBeVisible();
    await expect(profileGroup.getByRole('button', { name: 'Connect' })).not.toBeVisible();
    await expect(profileGroup.getByRole('button', { name: 'Stop' })).not.toBeVisible();

    const runtimeGroup = page
      .locator('.ant-typography-secondary')
      .filter({ hasText: /^Runtime$/ })
      .locator('..')
      .locator('..');
    await expect(runtimeGroup.getByRole('button', { name: 'Connect' })).toBeVisible();
    await expect(runtimeGroup.getByRole('button', { name: 'Stop' })).toBeVisible();
    await expect(runtimeGroup.getByRole('button', { name: 'Edit' })).not.toBeVisible();
    await expect(runtimeGroup.getByRole('button', { name: 'Duplicate' })).not.toBeVisible();
    await expect(runtimeGroup.getByRole('button', { name: 'Delete' })).not.toBeVisible();

    await openComputerTab(page, 'MCP Servers');
    await expect(page.getByRole('button', { name: 'Add Server' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Import Config' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Export Config' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Validate Schema' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Start All' })).not.toBeVisible();
    await expect(page.getByRole('button', { name: 'Stop All' })).not.toBeVisible();

    await openComputerTab(page, 'Runtime');
    await expect(page.getByRole('button', { name: 'Restart' })).toBeVisible();
    await expect(page.getByRole('button', { name: /Reload$/ })).not.toBeVisible();
    await expect(page.getByRole('button', { name: 'Start All' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Stop All' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Import Config' })).not.toBeVisible();
    await expect(page.getByRole('button', { name: 'Export Config' })).not.toBeVisible();

    await openComputerTab(page, 'Debug Panel');
    await expect(page.getByRole('tab', { name: 'Tools', exact: true })).toBeVisible();
    await expect(page.getByPlaceholder('Search tools...')).toBeVisible();
    await openComputerTab(page, 'Logs');
    await expect(page.getByRole('heading', { name: 'Logs', exact: true })).toBeVisible();
  });

  test('projects runtime status events into the active Computer view and refreshes consumers', async ({ page }) => {
    await openComputerTab(page, 'Runtime');
    await page.getByText('Advanced Runtime Diagnostics').click();
    await expect(page.getByRole('cell', { name: /Snapshot Revision\s*:\s*1/ })).toBeVisible();
    await expect(page.getByRole('cell', { name: /Capability Revision\s*:\s*1/ })).toBeVisible();
    await page.getByText('Advanced Runtime Diagnostics').click();

    await expect.poll(() => page.evaluate(() => {
      const invokes = (window as any).__TAURI_INVOKES__ as Array<{
        cmd: string;
        args?: { event?: string; handler?: number };
      }>;
      return invokes
        ?.filter((entry) => entry.cmd === 'plugin:event|listen'
          && entry.args?.event === 'computer-runtime-status')
        .at(-1)?.args?.handler;
    })).toEqual(expect.any(Number));

    await page.evaluate(() => {
      const invokes = (window as any).__TAURI_INVOKES__ as Array<{
        cmd: string;
        args?: { event?: string; handler?: number };
      }>;
      const handler = invokes
        .filter((entry) => entry.cmd === 'plugin:event|listen'
          && entry.args?.event === 'computer-runtime-status')
        .at(-1)?.args?.handler;
      if (handler === undefined) throw new Error('runtime event listener was not registered');

      (window as any).__TAURI_INTERNALS__.runCallback(handler, {
        event: 'computer-runtime-status',
        id: 1,
        payload: {
          instance_id: 'computer-a',
          cause: { kind: 'capability_revision_bumped', revision: 2 },
          snapshot: {
            incarnation: 1,
            generation: 1,
            snapshot_revision: 2,
            lifecycle: 'started',
            user_state: 'running',
            actions: {
              start: { enabled: false, disabled_reason: 'already_running' },
              stop: { enabled: true, disabled_reason: null },
              restart: { enabled: true, disabled_reason: null },
              connect: { enabled: true, disabled_reason: null },
              disconnect: { enabled: false, disabled_reason: 'connection_unavailable' },
              manage_mcp: { enabled: true, disabled_reason: null },
            },
            config_revision: 1,
            capability_revision: 2,
            mcp_servers: 1,
            active_mcp_servers: 1,
            tools: 1,
            skills: 0,
            problems: [],
            last_error: null,
            degraded_reason: null,
          },
          connection: { present: false, revision: 0, context: null },
        },
      });
    });

    await expect(page.getByRole('cell', { name: /Snapshot Revision\s*:\s*2/ })).toBeVisible();
    await expect(page.getByRole('cell', { name: /Capability Revision\s*:\s*2/ })).toBeVisible();
    await expect(page.getByText('Capability revision changed to 2')).toBeVisible();
    await expect.poll(async () => page.evaluate(() => (
      (window as any).__TAURI_INVOKES__ as Array<{ cmd: string; args?: unknown }>
    ).filter((entry) => entry.cmd === 'get_mcp_servers'
      && JSON.stringify(entry.args) === JSON.stringify({ instanceId: 'computer-a' })).length)).toBeGreaterThan(1);

    await page.evaluate(() => {
      const invokes = (window as any).__TAURI_INVOKES__ as Array<{
        cmd: string;
        args?: { event?: string; handler?: number };
      }>;
      const handler = invokes
        .filter((entry) => entry.cmd === 'plugin:event|listen'
          && entry.args?.event === 'computer-runtime-status')
        .at(-1)?.args?.handler;
      if (handler === undefined) throw new Error('runtime event listener was not registered');

      (window as any).__TAURI_INTERNALS__.runCallback(handler, {
        event: 'computer-runtime-status',
        id: 2,
        payload: {
          instance_id: 'computer-a',
          cause: {
            kind: 'mcp_diagnostic_changed',
            bundle_id: 'browser-mcp',
            operation: 'start',
            has_error: true,
          },
          snapshot: {
            incarnation: 1,
            generation: 1,
            snapshot_revision: 3,
            lifecycle: 'degraded',
            user_state: 'degraded',
            actions: {
              start: { enabled: false, disabled_reason: 'already_running' },
              stop: { enabled: true, disabled_reason: null },
              restart: { enabled: true, disabled_reason: null },
              connect: { enabled: false, disabled_reason: 'connection_unavailable' },
              disconnect: { enabled: false, disabled_reason: 'connection_unavailable' },
              manage_mcp: { enabled: false, disabled_reason: 'degraded' },
            },
            config_revision: 1,
            capability_revision: 2,
            mcp_servers: 1,
            active_mcp_servers: 0,
            tools: 0,
            skills: 0,
            problems: [{
              id: 'mcp:1:browser-mcp:start',
              source: 'mcp',
              operation: 'start',
              severity: 'degraded',
              affected_capabilities: [{
                kind: 'mcp_server',
                bundle_id: 'browser-mcp',
                name: 'Browser MCP',
              }],
              occurred_at: '2026-07-29T02:00:00Z',
              current: true,
              message: 'mcp_start_failed',
              recommended_actions: ['restart_runtime', 'view_logs'],
              technical_detail: 'process exited with code 1',
            }],
            last_error: null,
            degraded_reason: 'raw SDK detail must stay advanced',
          },
          connection: { present: false, revision: 0, context: null },
        },
      });
    });

    const runtimePanel = page.getByLabel('Runtime');
    await expect(runtimePanel.getByText('The Runtime is available, but an MCP server could not start.'))
      .toBeVisible();
    await expect(runtimePanel.getByText('Affected: MCP server Browser MCP')).toBeVisible();
    await expect(runtimePanel.getByText('process exited with code 1')).not.toBeVisible();
    await runtimePanel.getByRole('button', { name: 'View logs' }).click();
    await expect(page.getByRole('heading', { name: 'Logs', exact: true })).toBeVisible();
  });
});
