import { expect, test } from '@playwright/test';
import { setupInvokeMock } from '../fixtures/mock-invoke';

test.describe('Computer settings navigation and runtime boundary', () => {
  test.beforeEach(async ({ page }) => {
    await setupInvokeMock(page);
    await page.goto('/');
    await page.locator('.ant-layout-sider').getByText('Computer').click();
    await page.getByRole('button', { name: 'Computer A', exact: true }).click();
    await page.getByRole('button', { name: 'Open Computer settings' }).click();
  });

  test('opens all six settings modules and returns to the Computer runtime', async ({ page }) => {
    await expect(page.getByRole('heading', { name: 'Computer A Settings' })).toBeVisible();
    const navigation = page.getByRole('menu', { name: 'Computer settings sections' });
    const expectedSections = [
      'General',
      'Advanced',
      'Plugins',
      'MCP Servers',
      'Variables and credentials',
      'Connection Policy',
    ];
    for (const section of expectedSections) {
      await expect(navigation.getByText(section, { exact: true })).toBeVisible();
    }

    const expectNoRuntimeActions = async () => {
      for (const runtimeAction of ['Start', 'Stop', 'Restart', 'Connect', 'Disconnect']) {
        await expect(page.getByRole('button', {
          name: runtimeAction,
          exact: true,
        })).toHaveCount(0);
      }
      await expect(page.getByRole('button', { name: 'Start All' })).toHaveCount(0);
      await expect(page.getByRole('button', { name: 'Stop All' })).toHaveCount(0);
    };
    await expectNoRuntimeActions();

    await navigation.getByText('Advanced', { exact: true }).click();
    await expectNoRuntimeActions();
    await expect(page.getByRole('button', { name: /keyboard-helper/i })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Open saved directory' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Skill and plugin storage' }).getByText('/mock/computer_instances/computer-a/skill_home', {
      exact: true,
    })).toBeVisible();

    await navigation.getByText('Plugins', { exact: true }).click();
    await expectNoRuntimeActions();
    await expect(page.getByRole('tab', { name: 'Installed (2)' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Manage sources' })).toBeVisible();

    await navigation.getByText('MCP Servers', { exact: true }).click();
    await expectNoRuntimeActions();
    await expect(page.getByRole('button', { name: 'Add Server' })).toBeVisible();
    await page.getByRole('button', { name: 'More configuration actions' }).click();
    await expect(page.getByRole('menuitem', { name: 'Import Config' })).toBeVisible();
    await expect(page.getByRole('menuitem', { name: 'Export Config' })).toBeVisible();
    await expect(page.getByRole('menuitem', { name: 'Validate Schema' })).toBeVisible();
    await page.keyboard.press('Escape');

    await navigation.getByText('Variables and credentials', { exact: true }).click();
    await expectNoRuntimeActions();
    await expect(page.getByRole('button', { name: 'Add Input' })).toBeVisible();

    await navigation.getByText('Connection Policy', { exact: true }).click();
    await expectNoRuntimeActions();
    await expect(page.getByText('Connection Target', { exact: true })).toBeVisible();
    await expect(page.getByRole('combobox', {
      name: 'Select a Manager Robot',
    })).toBeVisible();
    await expect(page.getByRole('switch', { name: 'Auto Connect' })).toBeVisible();

    await page.getByRole('button', { name: 'Back to runtime workbench' }).click();
    await expect(page.getByLabel('Computer runtime workbench')).toBeVisible();
    await expect(page.getByRole('tab', { name: 'Workbench', exact: true })).toHaveAttribute('aria-selected', 'true');
    await expect(page.getByText('Advanced Runtime Diagnostics')).toHaveCount(0);
  });

  test('keeps directory editing in Advanced with one submit and no implicit save', async ({ page }, testInfo) => {
    const navigation = page.getByRole('menu', { name: 'Computer settings sections' });
    await navigation.getByText('Advanced', { exact: true }).click();
    const settings = page.getByLabel('Computer settings', { exact: true });
    await expect(settings.getByRole('textbox')).toHaveCount(0);
    await expect(settings.getByRole('button', { name: 'Save', exact: true })).toHaveCount(0);
    await expect(settings.getByRole('region', { name: 'Skill and plugin storage' }).getByText('Default directory', { exact: true })).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath('advanced-storage.png'), fullPage: true, animations: 'disabled' });
    await page.getByRole('button', { name: 'Change storage location', exact: true }).click();
    const editor = page.getByRole('dialog', { name: 'Change storage location' });
    await editor.getByLabel('Custom directory', { exact: true }).check();
    await editor.getByRole('textbox', { name: 'Custom directory path' }).fill('/draft/skills');
    await expect(editor.getByRole('button', { name: 'Save', exact: true })).toBeEnabled();
    await expect(editor.getByText(/Existing files stay in the original directory/)).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath('storage-editor.png'), fullPage: true, animations: 'disabled' });
    await editor.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(editor).toBeHidden();
    const commands = await page.evaluate(() => (window as typeof window & { __TAURI_INVOKES__?: Array<{ cmd: string }> }).__TAURI_INVOKES__ ?? []);
    expect(commands.some(({ cmd }) => cmd === 'update_computer_skill_home')).toBe(false);
  });

  test('starts with installed plugins and keeps source management and uninstall secondary', async ({ page }, testInfo) => {
    await page.setViewportSize({ width: 1440, height: 1000 });
    const navigation = page.getByRole('menu', { name: 'Computer settings sections' });
    await navigation.getByText('Plugins', { exact: true }).click();
    await expect(page.getByRole('tab', { name: 'Installed (2)' })).toHaveAttribute('aria-selected', 'true');
    await expect(page.getByRole('button', { name: 'Select plugin audit (plugin-2)' })).toBeVisible();
    await expect(page.getByText('https://example.com/acme.git')).toHaveCount(0);
    await page.screenshot({ path: testInfo.outputPath('installed-plugins.png'), fullPage: true, animations: 'disabled' });
    await page.getByRole('tab', { name: 'Discover plugins' }).click();
    await expect(page.getByRole('combobox', { name: 'Filter by source' })).toBeVisible();
    await page.getByRole('button', { name: 'Manage sources', exact: true }).click();
    const sources = page.getByRole('dialog', { name: 'Manage sources' });
    await expect(sources.getByText(/Syncing a source refreshes its catalog/)).toBeVisible();
    await expect(sources.getByRole('button', { name: 'Remove source', exact: true })).toBeEnabled();
    await sources.getByRole('button', { name: 'Remove source', exact: true }).click();
    const removal = page.getByRole('dialog', { name: 'Remove source acme?' });
    await expect(removal.getByText('audit:review', { exact: false })).toBeVisible();
    await expect(removal.getByText('plugin-tools', { exact: false })).toBeVisible();
    await expect(removal.getByRole('button', { name: 'Uninstall contents and remove source' })).toBeEnabled();
    await expect(removal).not.toHaveClass(/ant-zoom-(enter|appear)/);
    await page.screenshot({ path: testInfo.outputPath('source-removal-confirm.png'), fullPage: true, animations: 'disabled' });
    await removal.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(sources).not.toHaveClass(/ant-zoom-(enter|appear)/);
    await page.screenshot({ path: testInfo.outputPath('plugin-sources.png'), fullPage: true, animations: 'disabled' });
    await sources.getByRole('button', { name: 'Close', exact: true }).last().click();
    await page.getByRole('button', { name: 'More', exact: true }).click();
    await page.getByRole('menuitem', { name: 'Uninstall Plugin' }).click();
    const uninstall = page.getByRole('dialog', { name: 'Uninstall audit?' });
    await expect(uninstall).toBeVisible();
    await uninstall.getByRole('button', { name: 'Cancel', exact: true }).click();
    const commands = await page.evaluate(() => (window as typeof window & { __TAURI_INVOKES__?: Array<{ cmd: string }> }).__TAURI_INVOKES__ ?? []);
    expect(commands.some(({ cmd }) => ['uninstall_plugin', 'refresh_marketplace', 'remove_marketplace'].includes(cmd))).toBe(false);
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(1440);
  });

  test('keeps the Chinese plugin and storage flows readable', async ({ page }, testInfo) => {
    await page.setViewportSize({ width: 1440, height: 1000 });
    await page.getByRole('button', { name: 'Language', exact: true }).click();
    const navigation = page.getByRole('menu', { name: '计算机设置模块' });
    await navigation.getByText('高级', { exact: true }).click();
    await expect(page.getByRole('region', { name: '技能与插件存储位置' })).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath('advanced-storage-zh.png'), fullPage: true, animations: 'disabled' });
    await navigation.getByText('插件', { exact: true }).click();
    await expect(page.getByRole('tab', { name: '已安装（2）' })).toBeVisible();
    await expect(page.getByRole('button', { name: '管理来源', exact: true })).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath('plugins-zh.png'), fullPage: true, animations: 'disabled' });
    await page.getByRole('button', { name: '管理来源', exact: true }).click();
    const sources = page.getByRole('dialog', { name: '管理来源' });
    await expect(sources.getByRole('button', { name: /^关\s*闭$/ })).toBeVisible();
    await expect(sources).not.toHaveClass(/ant-zoom-(enter|appear)/);
    await page.screenshot({ path: testInfo.outputPath('plugin-sources-zh.png'), fullPage: true, animations: 'disabled' });
    await sources.getByRole('button', { name: '移除来源', exact: true }).click();
    const removal = page.getByRole('dialog', { name: '移除插件来源 acme？' });
    await expect(removal.getByText(/移除来源将级联卸载以下插件/)).toBeVisible();
    await expect(removal.getByText(/audit:review/)).toBeVisible();
    await expect(removal).not.toHaveClass(/ant-zoom-(enter|appear)/);
    await page.screenshot({ path: testInfo.outputPath('source-removal-confirm-zh.png'), fullPage: true, animations: 'disabled' });
    await removal.getByRole('button', { name: /^取\s*消$/ }).click();
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(1440);
  });

  test('reports a profile mutation as saved without inferring active Runtime effect', async ({
    page,
  }) => {
    const invokeCountBeforeSave = await page.evaluate(() => {
      const tauriWindow = window as typeof window & {
        __TAURI_INVOKES__?: Array<{ cmd: string }>;
      };
      return tauriWindow.__TAURI_INVOKES__?.length ?? 0;
    });
    await page.getByRole('textbox', { name: 'Name' }).fill('Renamed Computer');
    await page.getByRole('textbox', { name: 'Name' }).press('Tab');

    await expect(page.getByText('Saved', { exact: true }).first()).toBeVisible();
    await expect(page.getByText(/\bApplied\b/i)).toHaveCount(0);
    await expect(page.getByText(/\bEffective\b/i)).toHaveCount(0);

    const saveCommands = await page.evaluate((startIndex) => {
      const tauriWindow = window as typeof window & {
        __TAURI_INVOKES__?: Array<{ cmd: string }>;
      };
      return (tauriWindow.__TAURI_INVOKES__ ?? [])
        .slice(startIndex)
        .map(({ cmd }) => cmd);
    }, invokeCountBeforeSave);
    expect(saveCommands).toContain('patch_computer_general');
    expect(saveCommands.filter((command) => (
      /(^|_)(start|stop|restart|reload|apply)(_|$)/i.test(command)
    ))).toEqual([]);
  });

  test('keeps vertical settings navigation usable in a narrow window', async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 900 });
    const navigation = page.getByRole('menu', { name: 'Computer settings sections' });
    await expect(navigation).toBeVisible();
    await expect(navigation).toHaveClass(/ant-menu-inline/);

    const inputsMenuItem = navigation.getByRole('menuitem', { name: /Variables and credentials/ });
    await inputsMenuItem.focus();
    await expect(inputsMenuItem).toBeFocused();
    await inputsMenuItem.press('Enter');
    await expect(
      page.getByRole('heading', { name: 'Variables and credentials', exact: true }),
    ).toBeVisible();
    await expect(page.getByRole('button', { name: 'Add Input' })).toBeInViewport();

    await navigation.getByText('MCP Servers', { exact: true }).click();
    await expect(page.getByRole('button', { name: 'More configuration actions' })).toBeInViewport();
    await expect(page.getByRole('button', { name: 'Add Server' })).toBeInViewport();
    expect(await page.evaluate(() => document.documentElement.scrollWidth))
      .toBeLessThanOrEqual(await page.evaluate(() => document.documentElement.clientWidth));
  });

  test('opens the exact Plugin that authoritatively owns a read-only MCP declaration', async ({
    page,
  }) => {
    const navigation = page.getByRole('menu', { name: 'Computer settings sections' });
    await navigation.getByText('MCP Servers', { exact: true }).click();

    const settings = page.getByLabel('Computer settings', { exact: true });
    await expect(settings.getByText(
      'Managed by plugin audit from acme.',
    )).toBeVisible();
    await page.getByRole('button', { name: 'Manage audit' }).click();

    const previousOwner = page.getByRole('button', {
      name: 'Select plugin audit (plugin-old)',
    });
    const plugin = page.getByRole('button', {
      name: 'Select plugin audit (plugin-2)',
    });
    await expect(previousOwner).toHaveAttribute('aria-pressed', 'false');
    await expect(plugin).toHaveAttribute('aria-pressed', 'true');
    const pluginsPage = settings.locator('[data-page="computer-settings-plugins"]');
    await expect(pluginsPage.getByText('plugin-tools', { exact: true })).toBeVisible();
    await expect(pluginsPage.getByText('legacy-audit-mcp', { exact: true })).toHaveCount(0);

    await plugin.focus();
    await expect(plugin).toBeFocused();
    await plugin.press(' ');
    await expect(plugin).toHaveAttribute('aria-pressed', 'true');
  });
});

test('keeps saved changes visible in settings and applies one MCP from the workbench', async ({ page }, testInfo) => {
  await setupInvokeMock(page, {
    get_computer_config_adoption: {
      incarnation: 1, generation: 1, state: 'pending', unknown_resources: [],
      changes: [{ resource_id: 'user-tools', section: 'mcp', reason: 'mcp_config', action: 'restart_mcp', current_value: null, saved_value: null }],
    },
    restart_mcp_server_with_latest_config: null,
  });
  await page.goto('/');
  await page.locator('.ant-layout-sider').getByText('Computer').click();
  await page.getByRole('button', { name: 'Computer A', exact: true }).click();
  await page.getByRole('button', { name: 'Open Computer settings' }).click();
  await expect(page.getByRole('status').filter({ hasText: '1 configuration change not yet adopted' })).toBeVisible();
  await expect(page.getByRole('button', { name: /View configuration changes and actions/ })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Restart this MCP and adopt configuration' })).toHaveCount(0);
  await page.screenshot({ path: testInfo.outputPath('settings-pending.png'), fullPage: true, animations: 'disabled' });
  await page.getByRole('button', { name: 'View adoption and actions in the workbench', exact: true }).click();
  await page.getByRole('button', { name: /View configuration changes and actions/ }).click();
  await page.getByRole('button', { name: 'Restart this MCP and adopt configuration', exact: true }).click();
  const invokes = await page.evaluate(() => (window as typeof window & { __TAURI_INVOKES__?: Array<{cmd: string; args?: unknown}> }).__TAURI_INVOKES__ ?? []);
  expect(invokes).toContainEqual({ cmd: 'restart_mcp_server_with_latest_config', args: { instanceId: 'computer-a', bundleId: 'user-tools' } });
  expect(invokes.some(({ cmd }) => cmd === 'restart_computer_instance')).toBe(false);
});
