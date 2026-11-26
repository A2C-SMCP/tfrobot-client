import { expect, test } from '@playwright/test';
import { mockResponses, setupInvokeMock } from '../fixtures/mock-invoke';

test('opens the correct Computer and keeps common operations visible in both collection layouts', async ({ page, context }) => {
  const [first, second] = mockResponses.list_computer_instances as Array<Record<string, unknown>>;
  await page.setViewportSize({ width: 1440, height: 900 });
  await setupInvokeMock(page, {
    list_computer_instances: [first, { ...second, description: null }],
    get_computer_config_adoption: {
      incarnation: 1, generation: 1, state: 'pending', unknown_resources: [],
      changes: [{ resource_id: 'user-tools', section: 'mcp', reason: 'mcp_config', action: 'restart_mcp' }],
    },
  });
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.goto('/');
  const menu = page.locator('.ant-layout-sider');
  await menu.getByText('Computer', { exact: true }).click();
  const card = page.getByRole('group', { name: 'Computer A', exact: true });
  const secondCard = page.getByRole('group', { name: 'Second Computer', exact: true });
  const assertInformation = async () => {
    await expect(card.getByText('Primary test computer')).toBeVisible();
    await expect(card.getByText(/MCP servers/i)).toHaveCount(0);
    await expect(card.getByText('computer-a', { exact: true })).toHaveCount(0);
    const moreButton = card.getByRole('button', { name: 'More actions for Computer A', exact: true });
    await expect(moreButton).toBeVisible();
    const moreWidth = (await moreButton.boundingBox())!.width;
    for (const action of ['Stop', 'Connect']) {
      const button = card.getByRole('button', { name: action, exact: true });
      await expect(button).toBeVisible();
      await expect(button).toBeEmpty();
      expect((await button.boundingBox())!.width).toBe(moreWidth);
    }
    await expect(card.getByRole('button', { name: '1 configuration change not yet adopted' })).toBeVisible();
    const identityOffset = (computer: typeof card, name: string) => computer.getByRole('button', { name, exact: true })
      .evaluate((button) => button.getBoundingClientRect().top - button.closest('[role="group"]')!.getBoundingClientRect().top);
    expect(await identityOffset(card, 'Computer A')).toBeCloseTo(await identityOffset(secondCard, 'Second Computer'), 0);

    await secondCard.getByRole('button', { name: 'More actions for Second Computer' }).click();
    const actions = page.getByRole('menu', { name: 'More actions for Second Computer', exact: true });
    for (const action of ['Settings', 'Edit name and description', 'Copy ID', 'Duplicate', 'Delete']) {
      await expect(actions.getByRole('menuitem', { name: action, exact: true })).toBeVisible();
    }
    await actions.getByRole('menuitem', { name: 'Settings', exact: true }).click();
    await expect(page.getByRole('textbox', { name: 'Name', exact: true })).toHaveValue('Second Computer');
    await page.getByRole('menu', { name: 'Computer settings sections' }).getByText('MCP Servers', { exact: true }).click();
    await expect(page.getByRole('row', { name: /second-stdio-server/ })).toBeVisible();
    await page.getByRole('button', { name: 'Back to runtime workbench', exact: true }).click();
    await page.getByRole('button', { name: 'Back to Computers', exact: true }).click();
  };
  await assertInformation();
  await page.setViewportSize({ width: 960, height: 900 });
  expect(await secondCard.getByText('Second Computer', { exact: true }).evaluate((name) => name.clientWidth >= name.scrollWidth)).toBe(true);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.getByText('Icons', { exact: true }).click();
  await assertInformation();
  await menu.getByText('Settings', { exact: true }).click();
  await menu.getByText('Computer', { exact: true }).click();
  await expect(page.getByRole('radio', { name: /Icons/ })).toBeChecked();
  for (const width of [960, 961, 1360, 1361]) {
    await page.setViewportSize({ width, height: 900 });
    const name = secondCard.getByText('Second Computer', { exact: true });
    await expect(name).toBeVisible();
    expect(await name.evaluate((text) => text.clientWidth >= text.scrollWidth)).toBe(true);
  }
  await page.setViewportSize({ width: 961, height: 900 });
  const pending = card.getByRole('button', { name: '1 configuration change not yet adopted' });
  await expect(pending).toHaveText('1 change pending');
  expect(await pending.evaluate((caption) => caption.clientWidth >= caption.scrollWidth)).toBe(true);
  const more = card.getByRole('button', { name: 'More actions for Computer A' });
  const actions = page.getByRole('menu', { name: 'More actions for Computer A', exact: true });
  await more.press('Enter');
  await actions.getByRole('menuitem', { name: 'Copy ID', exact: true }).click();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe('computer-a');
  await expect(more).toBeFocused();
  await page.evaluate(async () => {
    await navigator.clipboard.writeText('clipboard before fallback');
    Object.defineProperty(navigator.clipboard, 'writeText', { value: undefined });
  });
  await more.press('Enter');
  await actions.getByRole('menuitem', { name: 'Copy ID', exact: true }).click();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe('computer-a');
  await more.press('Enter');
  await actions.getByRole('menuitem', { name: 'Delete', exact: true }).click();
  const confirmation = page.getByRole('dialog', { name: 'Delete this Computer?' });
  await expect(confirmation).toBeVisible();
  await confirmation.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(more).toBeFocused();
  const search = page.getByRole('searchbox', { name: 'Search by name or description' });
  await search.fill('primary test');
  await expect(secondCard).toHaveCount(0);
  await search.clear();
  await card.getByRole('button', { name: 'Computer A', exact: true }).click();
  await expect(page.getByRole('tab', { name: 'Workbench', exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Computer A', exact: true })).toBeVisible();
  await page.getByRole('tab', { name: 'Skills and tools', exact: true }).click();
  await expect(page.getByRole('region', { name: 'Skill preview' })).toBeInViewport();
});

test('uses the available column width for short names with short or missing descriptions', async ({ page }) => {
  const [template] = mockResponses.list_computer_instances as Array<Record<string, unknown>>;
  const examples = [
    { name: 'Videocut', description: '视频剪辑' },
    { name: 'eshop', description: '电商测试' },
    { name: '旅游规划', description: null },
    { name: '研究资料与项目归档工作站', description: '科研助手' },
  ];
  const longName = 'Research workspace '.repeat(12).trim();
  await setupInvokeMock(page, {
    list_computer_instances: [...examples, { name: longName, description: null }]
      .map((example, index) => ({ ...template, ...example, id: `computer-${index}` })),
  });
  await page.setViewportSize({ width: 1800, height: 900 });
  await page.goto('/');
  await page.locator('.ant-layout-sider').getByText('Computer', { exact: true }).click();

  for (const view of ['List', 'Icons']) {
    await page.getByText(view, { exact: true }).click();
    for (const { name } of examples) {
      const label = page.getByRole('button', { name, exact: true }).getByText(name, { exact: true });
      const { clientWidth, scrollWidth } = await label.evaluate((element) => ({
        clientWidth: element.clientWidth,
        scrollWidth: element.scrollWidth,
      }));
      expect(clientWidth, `${view}: ${name}`).toBeGreaterThanOrEqual(scrollWidth);
    }
    const overflowed = page.getByRole('button', { name: longName, exact: true }).getByText(longName, { exact: true });
    await expect(overflowed).toHaveAttribute('title', longName);
    expect(await overflowed.evaluate((element) => element.scrollWidth > element.clientWidth)).toBe(true);
  }
});

for (const width of [1280, 800, 375]) {
  test(`keeps the collection, settings form and modal usable at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await setupInvokeMock(page);
    await page.goto('/');
    await page.locator('.ant-layout-sider').getByText('Computer', { exact: true }).click();
    await expect(page.getByRole('button', { name: 'Computer A', exact: true })).toBeVisible();
    await expect(page.getByRole('group', { name: 'Computer A', exact: true }).getByText('Config adopted')).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    await page.getByText('Icons', { exact: true }).click();
    await expect(page.getByRole('group', { name: 'Computer A', exact: true }).getByRole('button', { name: 'Connect', exact: true })).toBeInViewport();
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    await page.getByRole('button', { name: 'Create Computer', exact: true }).click();
    await expect(page.getByRole('dialog', { name: 'Create Computer' })).toBeVisible();
    await expect(page.getByRole('dialog').getByRole('textbox', { name: 'Name', exact: true })).toBeVisible();
    await page.getByRole('dialog').getByRole('button', { name: 'Cancel' }).click();
    await page.getByRole('button', { name: 'Computer A', exact: true }).click();
    await page.getByRole('button', { name: 'Open Computer settings' }).click();
    await expect(page.getByRole('textbox', { name: 'Name', exact: true })).toHaveValue('Computer A');
    await expect(page.getByRole('button', { name: 'Save', exact: true })).toHaveCount(0);
    await page.getByRole('menu', { name: 'Computer settings sections' }).getByText('Advanced', { exact: true }).click();
    await expect(page.getByRole('spinbutton', { name: 'MCP startup concurrency' })).toHaveValue('5');
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
  });
}
