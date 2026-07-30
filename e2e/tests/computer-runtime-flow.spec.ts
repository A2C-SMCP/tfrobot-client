import { expect, test } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

test.describe('Computer configuration, runtime, and diagnostics boundaries', () => {
  test.beforeEach(async ({ page }) => {
    await setupInvokeMock(page);
    await page.goto('/');
    await page.locator('.ant-layout-sider').getByText('Computer').click();
    await page.getByRole('button', { name: 'Computer A', exact: true }).click();
  });

  test('keeps identity, config, runtime, and diagnostics operations semantically separate', async ({ page }) => {
    const workbench = page.getByLabel('Computer runtime workbench');
    await expect(workbench).toBeVisible();
    await expect(page.getByRole('tab', { name: 'Overview' })).toHaveCount(0);
    await expect(page.getByRole('tab', { name: 'Runtime' })).toHaveCount(0);
    const back = page.getByRole('button', { name: 'Back to Computers' });
    await expect(back).toBeVisible();
    await expect(back).toHaveText('');
    await back.hover();
    await expect(page.getByRole('tooltip', { name: 'Back to Computers' })).toBeVisible();
    const backHitTarget = await back.boundingBox();
    if (!backHitTarget) throw new Error('Back to Computers hit target is not measurable');
    expect(backHitTarget.width).toBeGreaterThanOrEqual(40);
    expect(backHitTarget.height).toBeGreaterThanOrEqual(40);
    await expect(page.getByRole('button', { name: 'Stop', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Connect', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Duplicate' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Copy ID' })).toBeVisible();
    await expect(workbench.getByRole('button', { name: /keyboard-helper/i })).toBeVisible();
    for (const configurationAction of [
      'Add Server',
      'Import Config',
      'Export Config',
      'Add Variable',
      'Open local directory',
      'Install Plugin',
      'Enable Plugin',
      'Disable Plugin',
      'Uninstall Plugin',
    ]) {
      await expect(page.getByRole('button', {
        name: configurationAction,
        exact: true,
      })).toHaveCount(0);
    }
    await expect(page.getByRole('combobox', {
      name: 'Select a Robot or Manual SMCP target',
    })).toHaveCount(0);
    await expect(page.getByText('Apply configuration', { exact: true })).toHaveCount(0);
    await expect(page.getByText('Reload required', { exact: true })).toHaveCount(0);

    await page.getByRole('button', { name: 'More Computer actions' }).click();
    await expect(page.getByText('Restart', { exact: true })).toBeVisible();
    await expect(page.getByText('View logs', { exact: true })).toHaveCount(0);
    await expect(page.getByText('Copy ID', { exact: true })).toHaveCount(0);
    await expect(page.getByText('Edit identity', { exact: true })).toHaveCount(0);
    await expect(page.getByText('Delete', { exact: true })).toBeVisible();
    await page.getByText('Delete', { exact: true }).click();
    await expect(page.getByRole('dialog', { name: 'Delete this Computer?' })).toBeVisible();
    await page.getByRole('button', { name: 'Cancel' }).click();

    await page.getByRole('button', { name: 'Open Computer settings' }).click();
    const settingsNavigation = page.getByRole('menu', {
      name: 'Computer settings sections',
    });
    await settingsNavigation.getByText('Skills', { exact: true }).click();
    await expect(page.getByRole('button', { name: /keyboard-helper/i })).toHaveCount(0);
    await expect(page.getByRole('button', {
      name: 'View active Skills in Computer',
    })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Open local directory' })).toBeVisible();

    await settingsNavigation.getByText('MCP Servers', { exact: true }).click();
    await expect(page.getByRole('button', { name: 'Add Server' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Import Config' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Export Config' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Validate Schema' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Start All' })).not.toBeVisible();
    await expect(page.getByRole('button', { name: 'Stop All' })).not.toBeVisible();

    await page.getByRole('button', { name: 'Back to Computer' }).click();
    await expect(page.getByLabel('Computer runtime workbench')).toBeVisible();
    await expect(page.getByRole('button', { name: /Reload$/ })).not.toBeVisible();
    await expect(page.getByRole('button', { name: 'Start All' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Stop All' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Import Config' })).not.toBeVisible();
    await expect(page.getByRole('button', { name: 'Export Config' })).not.toBeVisible();

    await page.getByText('Debug Panel', { exact: true }).click();
    await expect(page.getByRole('tab', { name: 'Tools', exact: true })).toBeVisible();
    await expect(page.getByPlaceholder('Search tools...')).toBeVisible();
    await page.getByRole('button', { name: /Logs/ }).click();
    await expect(page.getByRole('heading', { name: 'Logs', exact: true })).toBeVisible();
  });

  test('projects runtime status events into the active Computer view and refreshes consumers', async ({ page }) => {
    await page.getByText('Advanced Runtime Diagnostics').click();
    await expect(page.getByRole('cell', { name: /Snapshot Revision\s*:\s*1/ })).toBeVisible();
    await expect(page.getByRole('cell', { name: /Capability Revision\s*:\s*1/ })).toBeVisible();

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
    await page.getByText('Advanced Runtime Diagnostics').click();

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

    const runtimePanel = page.getByRole('region', { name: 'Runtime workbench' });
    await expect(runtimePanel.getByText('The Runtime is available, but an MCP server could not start.'))
      .toBeVisible();
    await expect(runtimePanel.getByText('Affected: MCP server Browser MCP')).toBeVisible();
    await expect(runtimePanel.getByText('process exited with code 1')).not.toBeVisible();
    await runtimePanel.getByRole('button', { name: 'View logs' }).click();
    await expect(page.getByRole('heading', { name: 'Logs', exact: true })).toBeVisible();
  });

  test('keeps the workbench accessible and responsive in a narrow desktop window', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 800, height: 760 });

    const workbench = page.getByLabel('Computer runtime workbench');
    await expect(workbench).toBeVisible();
    await expect(page.getByRole('button', { name: 'Back to Computers' })).toBeInViewport();
    await expect(page.getByRole('button', { name: 'Stop', exact: true })).toBeInViewport();
    await expect(page.getByRole('button', { name: 'Connect', exact: true })).toBeInViewport();
    await expect(page.getByRole('button', { name: 'Open Computer settings' })).toBeInViewport();
    await expect(page.getByRole('button', { name: 'More Computer actions' })).toBeInViewport();

    expect(await page.evaluate(() => document.documentElement.scrollWidth))
      .toBeLessThanOrEqual(await page.evaluate(() => document.documentElement.clientWidth));

    const skill = page.getByRole('button', { name: /keyboard-helper/i });
    await skill.focus();
    await page.keyboard.press('Enter');
    await expect(page.getByRole('heading', { name: 'Keyboard Helper' })).toBeVisible();

    const debugPanel = page.getByRole('button', { name: /Debug Panel/ });
    await debugPanel.focus();
    await debugPanel.press('Enter');
    await expect(page.getByRole('tab', { name: 'Tools', exact: true })).toBeVisible();
  });
});
