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
    await expect(page.getByRole('button', { name: /Reload$/ })).toBeVisible();
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
            actions: {
              can_start: false,
              can_stop: true,
              can_restart: true,
              can_reload: true,
              can_connect: true,
              can_disconnect: false,
              can_manage_mcp: true,
            },
            config_revision: 1,
            capability_revision: 2,
            mcp_servers: 1,
            active_mcp_servers: 1,
            tools: 1,
            skills: 0,
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
  });
});
