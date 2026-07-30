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
      'Skills',
      'Plugins & Marketplace',
      'MCP Servers',
      'Inputs',
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

    await navigation.getByText('Skills', { exact: true }).click();
    await expectNoRuntimeActions();
    await expect(page.getByRole('button', { name: /keyboard-helper/i })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'View active Skills in Computer' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Open local directory' })).toBeVisible();
    await expect(page.getByText('/mock/computer_instances/computer-a/skill_home', {
      exact: true,
    })).toBeVisible();

    await navigation.getByText('Plugins & Marketplace', { exact: true }).click();
    await expectNoRuntimeActions();
    await expect(page.getByRole('heading', { name: 'Marketplace', exact: true })).toBeVisible();

    await navigation.getByText('MCP Servers', { exact: true }).click();
    await expectNoRuntimeActions();
    await expect(page.getByRole('button', { name: 'Add Server' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Import Config' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Export Config' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Validate Schema' })).toBeVisible();

    await navigation.getByText('Inputs', { exact: true }).click();
    await expectNoRuntimeActions();
    await expect(page.getByRole('button', { name: 'Add Variable' })).toBeVisible();

    await navigation.getByText('Connection Policy', { exact: true }).click();
    await expectNoRuntimeActions();
    await expect(page.getByText('Connection Target', { exact: true })).toBeVisible();
    await expect(page.getByRole('combobox', {
      name: 'Select a Robot or Manual SMCP target',
    })).toBeVisible();
    await expect(page.getByRole('switch', { name: 'Auto Connect' })).toBeVisible();

    await page.getByRole('button', { name: 'Back to Computer' }).click();
    await expect(page.getByLabel('Computer runtime workbench')).toBeVisible();
    await expect(page.getByText('Advanced Runtime Diagnostics')).toBeVisible();
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
    const saveButton = page.getByRole('button', { name: /Save$/ });
    await saveButton.focus();
    await saveButton.press('Enter');

    await expect(page.getByText('Saved', { exact: true })).toBeVisible();
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
    expect(saveCommands).toContain('rename_computer_instance');
    expect(saveCommands.filter((command) => (
      /(^|_)(start|stop|restart|reload|apply)(_|$)/i.test(command)
    ))).toEqual([]);
  });

  test('keeps vertical settings navigation usable in a narrow window', async ({ page }) => {
    await page.setViewportSize({ width: 720, height: 900 });
    const navigation = page.getByRole('menu', { name: 'Computer settings sections' });
    await expect(navigation).toBeVisible();
    await expect(navigation).toHaveClass(/ant-menu-inline/);

    const inputsMenuItem = navigation.getByRole('menuitem', { name: /Inputs/ });
    await inputsMenuItem.focus();
    await expect(inputsMenuItem).toBeFocused();
    await inputsMenuItem.press('Enter');
    await expect(
      page.getByRole('heading', { name: 'Inputs', exact: true }),
    ).toBeVisible();
    await expect(page.getByRole('button', { name: 'Add Variable' })).toBeInViewport();

    await navigation.getByText('MCP Servers', { exact: true }).click();
    await expect(page.getByRole('button', { name: 'Import Config' })).toBeInViewport();
    await expect(page.getByRole('button', { name: 'Add Server' })).toBeInViewport();
    expect(await page.evaluate(() => document.documentElement.scrollWidth))
      .toBeLessThanOrEqual(await page.evaluate(() => document.documentElement.clientWidth));
  });

  test('opens the exact Plugin that authoritatively owns a read-only MCP declaration', async ({
    page,
  }) => {
    const navigation = page.getByRole('menu', { name: 'Computer settings sections' });
    await navigation.getByText('MCP Servers', { exact: true }).click();

    await expect(page.getByText(
      'Managed by plugin audit from acme. Use Marketplace to manage its lifecycle.',
    )).toBeVisible();
    await page.getByRole('button', { name: 'Manage audit' }).click();

    const marketplace = page.getByRole('button', {
      name: 'Select marketplace acme',
    });
    const previousOwner = page.getByRole('button', {
      name: 'Select plugin audit (plugin-old)',
    });
    const plugin = page.getByRole('button', {
      name: 'Select plugin audit (plugin-2)',
    });
    await expect(marketplace).toHaveAttribute('aria-pressed', 'true');
    await expect(previousOwner).toHaveAttribute('aria-pressed', 'false');
    await expect(plugin).toHaveAttribute('aria-pressed', 'true');
    await expect(page.getByText('plugin-tools', { exact: true })).toBeVisible();
    await expect(page.getByText('legacy-audit-mcp', { exact: true })).toHaveCount(0);

    await plugin.focus();
    await expect(plugin).toBeFocused();
    await plugin.press(' ');
    await expect(plugin).toHaveAttribute('aria-pressed', 'true');
  });
});
