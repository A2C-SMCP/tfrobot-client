import { NavigationMemoryProvider, NavigationScope } from '@/components/Navigation/NavigationMemory';
import { NavigationMemory } from '@/stores/navigationStore';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { StrictMode, useCallback, useState } from 'react';
import { ConfigProvider } from 'antd';
import { act, fireEvent, render, screen, waitFor, within } from '../helpers/render';
import { MarketplaceTab } from '@/components/Computer/MarketplaceTab';
import { useConfigAdoptionStore } from '@/stores/configAdoptionStore';
import { useSkillStore } from '@/stores/skillStore';

const mockedInvoke = vi.mocked(invoke);
const mockedOpen = vi.mocked(open);

vi.setConfig({ testTimeout: 30_000 });

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const supportedCapabilities = {
  computerLifecycleApiAvailable: true,
  supportedOperations: [
    'add_marketplace',
    'refresh_marketplace',
    'remove_marketplace',
    'update_marketplace',
    'install_plugin',
    'enable_plugin',
    'disable_plugin',
    'uninstall_plugin',
  ],
  requiredSdkApis: [],
  reason: 'supported',
};

describe('MarketplaceTab', () => {
  beforeEach(() => {
    useSkillStore.getState().reset();
    useConfigAdoptionStore.getState().reset();
    mockedInvoke.mockReset();
    mockedOpen.mockReset();
  });

  it('previews installed contents, cancels without deleting, and submits the confirmed impact list once', async () => {
    const source = { type: 'remoteGit' as const, displayGitUrl: 'https://example.com/acme.git' };
    const installed = { plugin: 'audit', version: '1.0.0', enabled: true, bundledSkills: ['audit:review'], bundledMcpServers: ['audit-mcp'] };
    const plan = { instanceId: 'computer-a', marketplace: 'acme', source, installedPlugins: [installed] };
    const preview = deferred<typeof plan>();
    let removed = false;
    mockedInvoke.mockImplementation(async (command) => {
      if (command === 'get_marketplace_governance') return {
        capabilities: supportedCapabilities,
        marketplaces: removed ? [] : [{ name: 'acme', source, status: 'known', message: null }],
        plugins: removed ? [] : [{ ...installed, marketplace: 'acme', installed: true, status: 'enabled', declared: null }],
      };
      if (command === 'list_skills') return [];
      if (command === 'get_marketplace_removal_plan') return preview.promise;
      if (command === 'remove_marketplace') { removed = true; return; }
    });
    render(<ConfigProvider theme={{ token: { motion: false } }}><MarketplaceTab instanceId="computer-a" /></ConfigProvider>);
    await screen.findByRole('tab', { name: 'Installed (1)' });
    fireEvent.click(screen.getByRole('button', { name: 'Manage sources' }));
    const sources = screen.getByRole('dialog', { name: 'Manage sources' });
    expect(within(sources).getByRole('button', { name: 'Remove source' })).toBeEnabled();
    fireEvent.click(within(sources).getByRole('button', { name: 'Remove source' }));
    const dialog = (await screen.findByText('Remove source acme?')).closest('[role=dialog]')! as HTMLElement;
    await waitFor(() => expect(dialog).toBeVisible());
    expect(within(dialog).getByRole('button', { name: 'Remove source' })).toBeDisabled();
    await act(async () => preview.resolve(plan));
    expect(await within(dialog).findByText('audit:review', { exact: false })).toBeVisible();
    expect(within(dialog).getByText(/audit-mcp/)).toBeVisible();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    expect(mockedInvoke).not.toHaveBeenCalledWith('remove_marketplace', expect.anything());
    fireEvent.click(within(sources).getByRole('button', { name: 'Remove source' }));
    const secondDialog = (await screen.findByText('Remove source acme?')).closest('[role=dialog]')! as HTMLElement;
    const confirm = await within(secondDialog).findByRole('button', { name: 'Uninstall contents and remove source' });
    await waitFor(() => expect(confirm).toBeEnabled());
    fireEvent.click(confirm);
    await waitFor(() => expect(mockedInvoke).toHaveBeenCalledWith('remove_marketplace', {
      instanceId: 'computer-a', marketplace: 'acme', confirmedPlan: plan,
    }));
    expect(mockedInvoke.mock.calls.filter(([cmd]) => cmd === 'remove_marketplace')).toHaveLength(1);
    await waitFor(() => expect(secondDialog).not.toBeVisible());
  });

  it('confirms removal of an empty source and keeps failures visible for a fresh preview', async () => {
    const source = { type: 'remoteGit' as const, displayGitUrl: 'https://example.com/acme.git' };
    const plan = { instanceId: 'computer-a', marketplace: 'acme', source, installedPlugins: [] };
    mockedInvoke.mockImplementation(async (command) => {
      if (command === 'get_marketplace_governance') return { capabilities: supportedCapabilities,
        marketplaces: [{ name: 'acme', source, status: 'known', message: null }], plugins: [] };
      if (command === 'list_skills') return [];
      if (command === 'get_marketplace_removal_plan') return plan;
      if (command === 'remove_marketplace') throw new Error('Source was retained; cleanup failed');
    });
    render(<ConfigProvider theme={{ token: { motion: false } }}><MarketplaceTab instanceId="computer-a" /></ConfigProvider>);
    await screen.findByText('No plugins installed for this Computer');
    fireEvent.click(screen.getByRole('button', { name: 'Manage sources' }));
    fireEvent.click(screen.getByRole('button', { name: 'Remove source' }));
    const dialog = (await screen.findByText('Remove source acme?')).closest('[role=dialog]')! as HTMLElement;
    await waitFor(() => expect(dialog).toBeVisible());
    expect(await within(dialog).findByText(/This source has no installed plugins or skills/)).toBeVisible();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Remove source' }));
    expect(await within(dialog).findByText(/Source was retained; cleanup failed/)).toBeVisible();
    expect(within(dialog).getByRole('button', { name: 'Remove source' })).toBeDisabled();
    expect(within(dialog).getByRole('button', { name: 'Reload affected items' })).toBeEnabled();
  });

  it('does not offer deletion when the removal preview cannot be read', async () => {
    mockedInvoke.mockImplementation(async (command) => {
      if (command === 'get_marketplace_governance') return { capabilities: supportedCapabilities,
        marketplaces: [{ name: 'acme', source: { type: 'remoteGit', displayGitUrl: null }, status: 'known' }], plugins: [] };
      if (command === 'list_skills') return [];
      if (command === 'get_marketplace_removal_plan') throw new Error('Preview unavailable');
    });
    render(<ConfigProvider theme={{ token: { motion: false } }}><MarketplaceTab instanceId="computer-a" /></ConfigProvider>);
    await screen.findByText('No plugins installed for this Computer');
    fireEvent.click(screen.getByRole('button', { name: 'Manage sources' }));
    fireEvent.click(screen.getByRole('button', { name: 'Remove source' }));
    const dialog = (await screen.findByText('Remove source acme?')).closest('[role=dialog]')! as HTMLElement;
    await waitFor(() => expect(dialog).toBeVisible());
    expect(await within(dialog).findByText('Preview unavailable')).toBeVisible();
    expect(within(dialog).getByRole('button', { name: 'Remove source' })).toBeDisabled();
    expect(mockedInvoke).not.toHaveBeenCalledWith('remove_marketplace', expect.anything());
  });

  it('does not render persistent SDK capability metadata and disables lifecycle actions when unsupported', async () => {
    mockedInvoke.mockResolvedValueOnce({
      capabilities: {
        computerLifecycleApiAvailable: false,
        supportedOperations: [],
        requiredSdkApis: ['Computer::install_plugin'],
        reason: 'SDK lifecycle unavailable',
      },
      marketplaces: [],
      plugins: [],
    }).mockResolvedValueOnce([]);

    render(<MarketplaceTab instanceId="computer-a" />);

    await screen.findByText('No plugins installed for this Computer');
    fireEvent.click(screen.getByRole('button', { name: 'Manage sources' }));
    expect(await screen.findByText('No plugin sources configured. Add a Git repository to discover plugins.')).toBeInTheDocument();

    expect(screen.getByText('Add plugin source').closest('button')).toBeDisabled();
    expect(screen.queryByText('SDK marketplace lifecycle is unavailable')).not.toBeInTheDocument();
    expect(screen.queryByText('Computer::install_plugin')).not.toBeInTheDocument();
    expect(screen.queryByText('Install scope')).not.toBeInTheDocument();
  });

  it('lists installed plugins across sources and shows the selected package contents', async () => {
    mockedInvoke.mockResolvedValueOnce({
      capabilities: supportedCapabilities,
      marketplaces: [
        { name: 'tf-market', source: { type: 'remoteGit', displayGitUrl: 'https://example.com/tf.git' }, status: 'known', message: 'lastUpdated=2026-07-03T06:08:14Z' },
        { name: 'acme', source: { type: 'remoteGit', displayGitUrl: 'https://example.com/acme.git' }, status: 'known', message: null },
      ],
      plugins: [
        {
          marketplace: 'tf-market',
          plugin: 'desktop-tools',
          pluginId: 'plugin-1',
          version: '1.0.0',
          installed: true,
          enabled: false,
          status: 'installed',
          bundledMcpServers: ['browser'],
          bundledSkills: ['summarizer'],
          declared: null,
          message: null,
        },
        {
          marketplace: 'acme',
          plugin: 'audit',
          pluginId: 'plugin-2',
          version: '1.0.0',
          installed: true,
          enabled: true,
          status: 'enabled',
          bundledMcpServers: ['audit-mcp'],
          bundledSkills: ['audit:code-review'],
          declared: null,
          message: 'Audit tools',
        },
      ],
    }).mockResolvedValueOnce([
      {
        name: 'summarizer',
        source: 'turingfocus-toolkit',
        path: '/tmp/summarizer',
        description: 'Summarizes selected text',
      },
    ]);

    render(<MarketplaceTab instanceId="computer-a" />);

    expect(await screen.findByRole('tab', { name: 'Installed (2)' })).toBeInTheDocument();
    expect(screen.queryByText('https://example.com/tf.git')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Select plugin audit (plugin-2)' })).toBeInTheDocument();
    expect(screen.getByText('Plugin Contents')).toBeInTheDocument();
    expect(screen.getAllByText('desktop-tools').length).toBeGreaterThan(0);
    expect(screen.getByRole('button', { name: 'Select plugin audit (plugin-2)' })).toBeInTheDocument();
    expect(await screen.findByText('browser')).toBeInTheDocument();
    expect(screen.getByText('summarizer')).toBeInTheDocument();

    const acmeSelection = screen.getByRole('button', { name: 'Select plugin audit (plugin-2)' });
    fireEvent.click(acmeSelection);

    expect(acmeSelection).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getAllByText('audit').length).toBeGreaterThan(0);
    expect(screen.getByRole('button', { name: 'Select plugin desktop-tools (plugin-1)' })).toBeInTheDocument();
    expect(screen.getByText('audit-mcp')).toBeInTheDocument();
    expect(screen.getByText('audit:code-review')).toBeInTheDocument();
  });

  it('opens the exact Marketplace Plugin requested by the Runtime workbench', async () => {
    mockedInvoke.mockResolvedValueOnce({
      capabilities: supportedCapabilities,
      marketplaces: [
        { name: 'tf-market', source: { type: 'remoteGit', displayGitUrl: null }, status: 'known', message: null },
        { name: 'acme', source: { type: 'remoteGit', displayGitUrl: null }, status: 'known', message: null },
      ],
      plugins: [
        {
          marketplace: 'tf-market',
          plugin: 'desktop-tools',
          pluginId: 'plugin-1',
          version: null,
          installed: true,
          enabled: true,
          status: 'enabled',
          bundledMcpServers: ['browser'],
          bundledSkills: [],
          declared: null,
          message: null,
        },
        {
          marketplace: 'acme',
          plugin: 'audit',
          pluginId: 'plugin-old',
          version: '0.9.0',
          installed: true,
          enabled: true,
          status: 'enabled',
          bundledMcpServers: ['legacy-audit-mcp'],
          bundledSkills: [],
          declared: null,
          message: 'Previous owner',
        },
        {
          marketplace: 'acme',
          plugin: 'audit',
          pluginId: 'plugin-2',
          version: '1.0.0',
          installed: true,
          enabled: true,
          status: 'enabled',
          bundledMcpServers: ['audit-mcp'],
          bundledSkills: [],
          declared: null,
          message: null,
        },
      ],
    }).mockResolvedValueOnce([]);

    const targetPlugin = {
      marketplace: 'acme',
      plugin: 'audit',
      pluginId: 'plugin-2',
    };
    function NavigationHarness() {
      const [target, setTarget] = useState<typeof targetPlugin | null>(null);
      const consumeTarget = useCallback(() => setTarget(null), []);
      return (
        <>
          <button type="button" onClick={() => setTarget(targetPlugin)}>
            Open audit Plugin
          </button>
          <span data-testid="plugin-navigation-state">{target ? 'pending' : 'idle'}</span>
          <MarketplaceTab
            instanceId="computer-a"
            targetPlugin={target}
            onTargetPluginConsumed={consumeTarget}
          />
        </>
      );
    }

    render(<NavigationHarness />);
    expect(await screen.findByText('browser')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Open audit Plugin' }));

    expect(await screen.findByRole('button', { name: 'Select plugin audit (plugin-2)' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getAllByText('audit').length).toBeGreaterThan(0);
    expect(screen.getByText('audit-mcp')).toBeInTheDocument();
    expect(screen.queryByText('legacy-audit-mcp')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Select plugin desktop-tools (plugin-1)' })).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByTestId('plugin-navigation-state')).toHaveTextContent('idle');
    });

    fireEvent.click(screen.getByRole('button', { name: 'Select plugin desktop-tools (plugin-1)' }));
    expect((await screen.findAllByText('desktop-tools')).length).toBeGreaterThan(0);

    fireEvent.click(screen.getByRole('button', { name: 'Open audit Plugin' }));
    expect(await screen.findByRole('button', { name: 'Select plugin audit (plugin-2)' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByText('audit-mcp')).toBeInTheDocument();
    expect(screen.queryByText('legacy-audit-mcp')).not.toBeInTheDocument();
  });

  it('does not fall back to a same-name Plugin when the authoritative ID is unavailable', async () => {
    mockedInvoke.mockResolvedValueOnce({
      capabilities: supportedCapabilities,
      marketplaces: [
        { name: 'acme', source: { type: 'remoteGit', displayGitUrl: null }, status: 'known', message: null },
      ],
      plugins: [{
        marketplace: 'acme',
        plugin: 'audit',
        pluginId: 'plugin-old',
        version: '0.9.0',
        installed: true,
        enabled: true,
        status: 'enabled',
        bundledMcpServers: ['legacy-audit-mcp'],
        bundledSkills: [],
        declared: null,
        message: null,
      }],
    }).mockResolvedValueOnce([]);
    const onTargetPluginConsumed = vi.fn();

    render(
      <MarketplaceTab
        instanceId="computer-a"
        targetPlugin={{
          marketplace: 'acme',
          plugin: 'audit',
          pluginId: 'plugin-2',
        }}
        onTargetPluginConsumed={onTargetPluginConsumed}
      />,
    );

    expect(await screen.findByText('legacy-audit-mcp')).toBeInTheDocument();
    expect(onTargetPluginConsumed).not.toHaveBeenCalled();
  });

  it('preserves unknown, empty, declared, and installed capability semantics', async () => {
    mockedInvoke.mockResolvedValueOnce({
      capabilities: supportedCapabilities,
      marketplaces: [
        { name: 'tf-market', source: { type: 'remoteGit', displayGitUrl: 'https://example.com/tf.git' }, status: 'known', message: null },
      ],
      plugins: [
        {
          marketplace: 'tf-market', plugin: 'unknown-plugin', pluginId: 'unknown@tf-market', version: null,
          installed: false, enabled: false, status: 'available', bundledMcpServers: [], bundledSkills: [],
          declared: null, message: null,
        },
        {
          marketplace: 'tf-market', plugin: 'empty-plugin', pluginId: 'empty@tf-market', version: null,
          installed: false, enabled: false, status: 'available', bundledMcpServers: [], bundledSkills: [],
          declared: { version: null, description: null, mcpServers: [], skills: [] }, message: null,
        },
        {
          marketplace: 'tf-market', plugin: 'declared-plugin', pluginId: 'declared@tf-market', version: '1.0.0',
          installed: false, enabled: false, status: 'available',
          bundledMcpServers: ['catalog-mcp'], bundledSkills: ['catalog-skill'],
          declared: {
            version: '1.0.0', description: 'Catalog declaration',
            mcpServers: ['catalog-mcp'], skills: ['catalog-skill'],
          },
          message: null,
        },
        {
          marketplace: 'tf-market', plugin: 'installed-plugin', pluginId: 'installed@tf-market', version: '2.0.0',
          installed: true, enabled: true, status: 'enabled',
          bundledMcpServers: ['actual-mcp'], bundledSkills: ['actual-skill'],
          declared: {
            version: '1.0.0', description: 'Stale catalog declaration',
            mcpServers: ['declared-only-mcp'], skills: ['declared-only-skill'],
          },
          message: null,
        },
      ],
    }).mockResolvedValueOnce([]);

    render(<MarketplaceTab instanceId="computer-a" />);

    fireEvent.click(await screen.findByRole('tab', { name: 'Discover plugins' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Select plugin unknown-plugin (unknown@tf-market)' }));
    expect(await screen.findByText('Bundled skill capabilities are unknown')).toBeInTheDocument();
    expect(screen.getByText('Bundled MCP server capabilities are unknown')).toBeInTheDocument();

    fireEvent.click(screen.getByText('empty-plugin'));
    expect(await screen.findByText('No bundled skills')).toBeInTheDocument();
    expect(screen.getByText('No bundled MCP servers')).toBeInTheDocument();

    fireEvent.click(screen.getByText('declared-plugin'));
    expect(await screen.findByText('catalog-skill')).toBeInTheDocument();
    expect(screen.getByText('catalog-mcp')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Select plugin installed-plugin (installed@tf-market)' }));
    expect(await screen.findByText('actual-skill')).toBeInTheDocument();
    expect(screen.getByText('actual-mcp')).toBeInTheDocument();
    expect(screen.queryByText('declared-only-skill')).not.toBeInTheDocument();
    expect(screen.queryByText('declared-only-mcp')).not.toBeInTheDocument();
  });

  it.each([
    { type: 'remoteGit', displayGitUrl: 'https://example.com/private.git' },
    { type: 'localGit', path: '/tmp/local-market' },
  ])('shows source details without an unavailable edit action ($type)', async (source) => {
    mockedInvoke.mockResolvedValueOnce({
      capabilities: { ...supportedCapabilities, supportedOperations: supportedCapabilities.supportedOperations.filter((operation) => operation !== 'update_marketplace') },
      marketplaces: [{ name: 'sdk-name', source, status: 'known', message: null }],
      plugins: [],
    }).mockResolvedValueOnce([]);

    render(<ConfigProvider theme={{ token: { motion: false } }}><MarketplaceTab instanceId="computer-a" /></ConfigProvider>);
    await screen.findByText('No plugins installed for this Computer');
    fireEvent.click(screen.getByRole('button', { name: 'Manage sources' }));
    const dialog = screen.getByRole('dialog', { name: 'Manage sources' });
    await waitFor(() => expect(within(dialog).getByText('sdk-name')).toBeVisible());
    expect(within(dialog).getByText(source.type === 'localGit' ? source.path! : source.displayGitUrl!)).toBeVisible();
    expect(within(dialog).queryByRole('button', { name: 'Edit' })).not.toBeInTheDocument();
    expect(within(dialog).getByRole('button', { name: 'Sync plugin catalog' })).toBeEnabled();
    expect(within(dialog).getByRole('button', { name: 'Remove source' })).toBeEnabled();
  });

  it('rejects file URLs in remote repository mode before invoking Tauri', async () => {
    mockedInvoke
      .mockResolvedValueOnce({
        capabilities: supportedCapabilities,
        marketplaces: [],
        plugins: [],
      })
      .mockResolvedValueOnce([]);

    render(<MarketplaceTab instanceId="computer-a" />);

    await screen.findByText('No plugins installed for this Computer');
    fireEvent.click(screen.getByRole('button', { name: 'Manage sources' }));
    expect(await screen.findByText('No plugin sources configured. Add a Git repository to discover plugins.')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Add plugin source').closest('button')!);
    expect(screen.queryByLabelText('Source name')).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('Git URL'), {
      target: { value: 'file:///tmp/local-market' },
    });
    const addButtons = screen.getAllByText('Add plugin source');
    fireEvent.click(addButtons[addButtons.length - 1].closest('button')!);

    expect(await screen.findByText(/must be added as a local repository/i)).toBeInTheDocument();
    expect(mockedInvoke).not.toHaveBeenCalledWith(
      'add_marketplace',
      expect.anything(),
    );
  });

  it('selects one local directory, preserves it when selection is cancelled, and submits a structured source', async () => {
    mockedOpen
      .mockResolvedValueOnce('/tmp/Marketplace 空格')
      .mockResolvedValueOnce(null);
    mockedInvoke
      .mockResolvedValueOnce({
        capabilities: supportedCapabilities,
        marketplaces: [],
        plugins: [],
      })
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce({
        capabilities: supportedCapabilities,
        marketplaces: [],
        plugins: [],
      })
      .mockResolvedValueOnce([]);

    render(<MarketplaceTab instanceId="computer-a" />);

    await screen.findByText('No plugins installed for this Computer');
    fireEvent.click(screen.getByRole('button', { name: 'Manage sources' }));
    expect(await screen.findByText('No plugin sources configured. Add a Git repository to discover plugins.')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Add plugin source').closest('button')!);
    expect(screen.queryByLabelText('Source name')).not.toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('Local repository'));
    expect(screen.getByLabelText('Local repository')).toBeChecked();
    fireEvent.click(screen.getByRole('button', { name: 'Choose folder' }));
    await waitFor(() => {
      expect(screen.getByLabelText('Local repository path')).toHaveValue('/tmp/Marketplace 空格');
    });
    expect(mockedOpen).toHaveBeenCalledWith({ directory: true, multiple: false });

    fireEvent.click(screen.getByRole('button', { name: 'Choose folder' }));
    await waitFor(() => expect(mockedOpen).toHaveBeenCalledTimes(2));
    expect(screen.getByLabelText('Local repository path')).toHaveValue('/tmp/Marketplace 空格');

    const addButtons = screen.getAllByText('Add plugin source');
    fireEvent.click(addButtons[addButtons.length - 1].closest('button')!);

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('add_marketplace', {
        instanceId: 'computer-a',
        request: {
          source: { type: 'localGit', path: '/tmp/Marketplace 空格' },
        },
      });
    });
  });

  it('refreshes the selected Marketplace through the capability-gated lifecycle action', async () => {
    const governance = {
      capabilities: supportedCapabilities,
      marketplaces: [
        {
          name: 'tf-market',
          source: { type: 'remoteGit', displayGitUrl: 'https://example.com/tf.git' },
          status: 'known',
          message: null,
        },
      ],
      plugins: [],
    };
    mockedInvoke
      .mockResolvedValueOnce(governance)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(governance)
      .mockResolvedValueOnce([]);

    render(<MarketplaceTab instanceId="computer-a" />);

    await screen.findByText('No plugins installed for this Computer');
    fireEvent.click(screen.getByRole('button', { name: 'Manage sources' }));
    const refresh = await screen.findByRole('button', { name: 'Sync plugin catalog' });
    fireEvent.click(refresh);

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('refresh_marketplace', {
        instanceId: 'computer-a',
        marketplace: 'tf-market',
      });
    });
    expect(await screen.findByText('Plugin catalog synced')).toBeInTheDocument();
  });

  it('reports Marketplace refresh failures without changing the selection', async () => {
    mockedInvoke
      .mockResolvedValueOnce({
        capabilities: supportedCapabilities,
        marketplaces: [
          {
            name: 'tf-market',
            source: { type: 'remoteGit', displayGitUrl: 'https://example.com/tf.git' },
            status: 'known',
            message: null,
          },
        ],
        plugins: [],
      })
      .mockResolvedValueOnce([])
      .mockRejectedValueOnce(new Error('refresh failed'));

    render(<MarketplaceTab instanceId="computer-a" />);

    await screen.findByText('No plugins installed for this Computer');
    fireEvent.click(screen.getByRole('button', { name: 'Manage sources' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Sync plugin catalog' }));

    expect(await screen.findByText('refresh failed')).toBeInTheDocument();
    expect(screen.getByText('tf-market')).toBeInTheDocument();
  });

  it('closes the form and shows clone status while adding a marketplace in the background', async () => {
    const addMarketplace = deferred<void>();
    mockedInvoke
      .mockResolvedValueOnce({
        capabilities: supportedCapabilities,
        marketplaces: [],
        plugins: [],
      })
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce({
        capabilities: supportedCapabilities,
        marketplaces: [],
        plugins: [],
      })
      .mockResolvedValueOnce([])
      .mockReturnValueOnce(addMarketplace.promise as never)
      .mockResolvedValueOnce({
        capabilities: supportedCapabilities,
        marketplaces: [],
        plugins: [],
      })
      .mockResolvedValueOnce({
        capabilities: supportedCapabilities,
        marketplaces: [
          { name: 'tf-market', source: { type: 'remoteGit', displayGitUrl: 'https://example.com/tf.git' }, status: 'known', message: null },
        ],
        plugins: [],
      })
      .mockResolvedValueOnce([]);

    render(
      <StrictMode>
        <MarketplaceTab instanceId="computer-a" />
      </StrictMode>,
    );

    await screen.findByText('No plugins installed for this Computer');
    fireEvent.click(screen.getByRole('button', { name: 'Manage sources' }));
    expect(await screen.findByText('No plugin sources configured. Add a Git repository to discover plugins.')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Add plugin source').closest('button')!);
    expect(screen.queryByLabelText('Source name')).not.toBeInTheDocument();
    expect(screen.getByLabelText('Source type').closest('.ant-modal')).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('Git URL'), { target: { value: 'https://example.com/tf.git' } });
    const addButtons = screen.getAllByText('Add plugin source');
    fireEvent.click(addButtons[addButtons.length - 1].closest('button')!);

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('add_marketplace', {
        instanceId: 'computer-a',
        request: {
          source: { type: 'remoteGit', gitUrl: 'https://example.com/tf.git' },
        },
      });
    });
    await waitFor(() => {
      expect(screen.getByLabelText('Source type').closest('.ant-modal'))
        .toHaveClass('ant-zoom-leave');
    });
    expect(screen.getByText('Cloning plugin source…')).toBeInTheDocument();
    expect(screen.queryByText(/trust/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/confirm/i)).not.toBeInTheDocument();

    await act(async () => {
      await useSkillStore.getState().fetchMarketplaceGovernance('computer-a');
    });
    expect(screen.getByText('Reload list').closest('button')).toBeDisabled();
    expect(screen.getAllByText('Add plugin source')[0].closest('button')).toBeDisabled();

    addMarketplace.resolve();

    expect(await screen.findByText('Plugin source added')).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.queryByText('Cloning plugin source…')).not.toBeInTheDocument();
    });
  });

  it('does not show completion from a previous computer after switching instances', async () => {
    const addMarketplace = deferred<void>();
    mockedInvoke.mockImplementation((command, args) => {
      if (command === 'add_marketplace' && (args as { instanceId: string }).instanceId === 'computer-a') {
        return addMarketplace.promise as never;
      }
      if (command === 'get_marketplace_governance') {
        return Promise.resolve({
          capabilities: supportedCapabilities,
          marketplaces: [],
          plugins: [],
        }) as never;
      }
      if (command === 'list_skills') return Promise.resolve([]) as never;
      return Promise.resolve() as never;
    });

    const view = render(<MarketplaceTab instanceId="computer-a" />);

    await screen.findByText('No plugins installed for this Computer');
    fireEvent.click(screen.getByRole('button', { name: 'Manage sources' }));
    expect(await screen.findByText('No plugin sources configured. Add a Git repository to discover plugins.')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Add plugin source').closest('button')!);
    expect(screen.queryByLabelText('Source name')).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('Git URL'), { target: { value: 'https://example.com/tf.git' } });
    const addButtons = screen.getAllByText('Add plugin source');
    fireEvent.click(addButtons[addButtons.length - 1].closest('button')!);
    expect(await screen.findByText('Cloning plugin source…')).toBeInTheDocument();

    view.rerender(<MarketplaceTab instanceId="computer-b" />);
    await waitFor(() => {
      expect(useSkillStore.getState().activeInstanceId).toBe('computer-b');
    });

    await act(async () => {
      addMarketplace.resolve();
      await addMarketplace.promise;
    });
    await waitFor(() => {
      expect(useSkillStore.getState().recordsByInstanceId['computer-a'].marketplaceOperation)
        .toBeNull();
    });

    expect(screen.queryByText('Plugin source added')).not.toBeInTheDocument();
  });

  it('enables only operations exposed by SDK capabilities', async () => {
    mockedInvoke.mockResolvedValueOnce({
      capabilities: {
        computerLifecycleApiAvailable: true,
        supportedOperations: ['enable_plugin'],
        requiredSdkApis: [],
        reason: 'partial support',
      },
      marketplaces: [
        { name: 'tf-market', source: { type: 'remoteGit', displayGitUrl: 'https://example.com/tf.git' }, status: 'known', message: null },
      ],
      plugins: [
        {
          marketplace: 'tf-market',
          plugin: 'desktop-tools',
          pluginId: 'plugin-1',
          version: '1.0.0',
          installed: true,
          enabled: false,
          status: 'installed',
          bundledMcpServers: [],
          bundledSkills: [],
          declared: null,
          message: null,
        },
      ],
    }).mockResolvedValueOnce([]);

    render(<MarketplaceTab instanceId="computer-a" />);

    expect(await screen.findByText('desktop-tools')).toBeInTheDocument();
    expect(screen.getByText('Enable Plugin').closest('button')).toBeEnabled();
    expect(screen.queryByText('Refresh Marketplace')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Manage sources' }));
    expect(screen.getByRole('button', { name: 'Remove source' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Add plugin source' })).toBeDisabled();
  });

  it('keeps one plugin enable pending while the global runtime input bridge owns prompting', async () => {
    const governance = {
      capabilities: supportedCapabilities,
      marketplaces: [
        { name: 'acme', source: { type: 'remoteGit', displayGitUrl: 'https://example.com/acme.git' }, status: 'known', message: null },
      ],
      plugins: [
        {
          marketplace: 'acme',
          plugin: 'audit',
          pluginId: 'audit@acme',
          version: '1.0.0',
          installed: true,
          enabled: false,
          status: 'installed',
          bundledMcpServers: ['audit-mcp'],
          bundledSkills: [],
          declared: null,
          message: null,
        },
      ],
    };
    let enableAttempts = 0;
    let enableCompleted = false;
    const enable = deferred<void>();
    mockedInvoke.mockImplementation(async (command) => {
      if (command === 'get_marketplace_governance') {
        return !enableCompleted ? governance : {
          ...governance,
          plugins: [{ ...governance.plugins[0], enabled: true, status: 'enabled' }],
        };
      }
      if (command === 'list_skills') return [];
      if (command === 'enable_plugin') {
        enableAttempts += 1;
        await enable.promise;
        enableCompleted = true;
        return undefined;
      }
      throw new Error(`Unexpected invoke command: ${command}`);
    });

    render(<MarketplaceTab instanceId="computer-a" />);

    fireEvent.click((await screen.findByText('Enable Plugin')).closest('button')!);
    await waitFor(() => {
      expect(enableAttempts).toBe(1);
    });
    expect(screen.queryByText('Secret required to start')).not.toBeInTheDocument();
    expect(mockedInvoke).not.toHaveBeenCalledWith('upsert_input_entry', expect.anything());
    await act(async () => {
      enable.resolve();
      await enable.promise;
    });
    await waitFor(() => {
      expect(screen.getAllByText('Enabled').length).toBeGreaterThan(0);
    });
    expect(mockedInvoke.mock.calls.filter(([command]) => command === 'enable_plugin')).toHaveLength(1);
  });

  it('previews an enabled plugin skill from the details pane', async () => {
    mockedInvoke
      .mockResolvedValueOnce({
        capabilities: supportedCapabilities,
        marketplaces: [
          { name: 'tf-market', source: { type: 'remoteGit', displayGitUrl: 'https://example.com/tf.git' }, status: 'known', message: null },
        ],
        plugins: [
          {
            marketplace: 'tf-market',
            plugin: 'desktop-tools',
            pluginId: 'plugin-1',
            version: '1.0.0',
            installed: true,
            enabled: true,
            status: 'enabled',
            bundledMcpServers: ['browser'],
            bundledSkills: ['summarizer'],
            declared: null,
            message: null,
        },
      ],
      })
      .mockResolvedValueOnce([
        {
          name: 'summarizer',
          source: 'turingfocus-toolkit',
          path: '/tmp/summarizer',
          description: 'Summarizes selected text',
        },
      ])
      .mockResolvedValueOnce({
        name: 'summarizer',
        relPath: 'SKILL.md',
        mimeType: 'text/markdown',
        totalSize: 24,
        sha256: 'abc',
        isEntry: true,
        isText: true,
        body: '# Summarizer\n\nSummarize text.',
      });

    render(<MarketplaceTab instanceId="computer-a" />);

    fireEvent.click(await screen.findByRole('button', { name: /summarizer/ }));

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('get_skill', {
        instanceId: 'computer-a',
        name: 'summarizer',
        relPath: null,
      });
    });
    expect(await screen.findByText('Summarizer')).toBeInTheDocument();
    expect(screen.getByText('Summarize text.')).toBeInTheDocument();
  });

  it('does not start the second refresh read after its page is released', async () => {
    const governance = { capabilities: supportedCapabilities, marketplaces: [], plugins: [] };
    let finish!: (value: unknown) => void;
    const initialLoad = deferred<typeof governance>();
    let refreshing = false;
    mockedInvoke.mockImplementation(async (command) => {
      if (command === 'get_marketplace_governance') return refreshing ? new Promise((resolve) => { finish = resolve; }) : initialLoad.promise;
      if (command === 'list_skills') return [];
      throw new Error(`Unexpected command: ${command}`);
    });
    const view = render(<MarketplaceTab instanceId="computer-a" />);
    const refreshButton = screen.getByRole('button', { name: 'Reload list' });
    // The initial request keeps reload disabled while a skeleton is shown.
    expect(refreshButton).toBeDisabled();
    await act(async () => { initialLoad.resolve(governance); });
    expect(refreshButton).toBeEnabled();
    const count = mockedInvoke.mock.calls.filter(([command]) => command === 'list_skills').length;
    refreshing = true;
    fireEvent.click(refreshButton);
    await waitFor(() => expect(finish).toBeDefined());
    view.unmount();
    useSkillStore.getState().reset();
    await act(async () => { finish(governance); });
    expect(mockedInvoke.mock.calls.filter(([command]) => command === 'list_skills')).toHaveLength(count);
    expect(useSkillStore.getState().recordsByInstanceId).toEqual({});
  });

  it('reloads the selected plugin skill after its Computer subtree is released', async () => {
    const governance = { capabilities: supportedCapabilities,
      marketplaces: [{ name: 'tf-market', source: { type: 'remoteGit', displayGitUrl: 'https://example.com/tf.git' }, status: 'known', message: null }],
      plugins: [{ marketplace: 'tf-market', plugin: 'desktop-tools', pluginId: 'plugin-1', version: '1.0.0',
        installed: true, enabled: true, status: 'enabled', bundledMcpServers: [], bundledSkills: ['summarizer'], declared: null, message: null }],
    };
    mockedInvoke.mockImplementation(async (command) => {
      if (command === 'get_marketplace_governance') return governance;
      if (command === 'list_skills') return [];
      if (command === 'get_skill') return { name: 'summarizer', isText: true, body: '# Restored preview' };
      throw new Error(`Unexpected command: ${command}`);
    });
    const memory = new NavigationMemory();
    const tree = (id: string) => <NavigationMemoryProvider memory={memory}>
      <NavigationScope id={`computer:${id}`} key={id}><MarketplaceTab instanceId={id} /></NavigationScope>
    </NavigationMemoryProvider>;
    const view = render(tree('computer-a'));
    fireEvent.click(await screen.findByRole('button', { name: /summarizer/ }));
    expect(await screen.findByText('Restored preview')).toBeInTheDocument();
    view.rerender(tree('computer-b'));
    await screen.findByText('summarizer');
    view.rerender(tree('computer-a'));
    expect(await screen.findByText('Restored preview')).toBeInTheDocument();
    expect(mockedInvoke.mock.calls.filter(([command, args]) => command === 'get_skill' && (args as { instanceId?: string } | undefined)?.instanceId === 'computer-a')).toHaveLength(2);
  });

  it('renders skill preview errors with a readable alert message', async () => {
    mockedInvoke
      .mockResolvedValueOnce({
        capabilities: supportedCapabilities,
        marketplaces: [
          { name: 'tf-market', source: { type: 'remoteGit', displayGitUrl: 'https://example.com/tf.git' }, status: 'known', message: null },
        ],
        plugins: [
          {
            marketplace: 'tf-market',
            plugin: 'desktop-tools',
            pluginId: 'plugin-1',
            version: '1.0.0',
            installed: true,
            enabled: true,
            status: 'enabled',
            bundledMcpServers: [],
            bundledSkills: ['summarizer'],
            declared: null,
            message: null,
        },
      ],
      })
      .mockResolvedValueOnce([])
      .mockRejectedValueOnce({
        message: 'Skill not found: summarizer',
      });

    render(<MarketplaceTab instanceId="computer-a" />);

    fireEvent.click(await screen.findByRole('button', { name: /summarizer/ }));

    expect(await screen.findByText('Skill not found: summarizer')).toBeInTheDocument();
    expect(screen.queryByText('[object Object]')).not.toBeInTheDocument();
  });

  it('does not render stale marketplace governance from a different active instance', () => {
    mockedInvoke.mockReturnValue(new Promise(() => undefined) as any);
    useSkillStore.setState((state) => ({
      ...state,
      activeInstanceId: 'computer-a',
      capabilities: supportedCapabilities,
      marketplaces: [
        { name: 'stale-market', source: { type: 'remoteGit', displayGitUrl: 'https://example.com/stale.git' }, status: 'known', message: null },
      ],
      plugins: [
        {
          marketplace: 'stale-market',
          plugin: 'stale-plugin',
          pluginId: 'plugin-1',
          version: '1.0.0',
          installed: true,
          enabled: false,
          status: 'installed',
          bundledMcpServers: [],
          bundledSkills: [],
          declared: null,
          message: null,
        },
      ],
      recordsByInstanceId: {
        'computer-a': {
          skills: [],
          selectedSkillName: null,
          selectedSkill: null,
          governance: {
            capabilities: supportedCapabilities,
            marketplaces: [
              { name: 'stale-market', source: { type: 'remoteGit', displayGitUrl: 'https://example.com/stale.git' }, status: 'known', message: null },
            ],
            plugins: [
              {
                marketplace: 'stale-market',
                plugin: 'stale-plugin',
                pluginId: 'plugin-1',
                version: '1.0.0',
                installed: true,
                enabled: false,
                status: 'installed',
                bundledMcpServers: [],
                bundledSkills: [],
                declared: null,
                message: null,
              },
            ],
          },
          loadingSkills: false,
          loadingSkill: false,
          loadingMarketplace: false,
          marketplaceOperation: null,
          error: null,
          skillError: null,
          marketplaceError: null,
          skillsRequestId: 0,
          skillRequestId: 0,
          marketplaceRequestId: 1,
        },
      },
    }));

    render(<MarketplaceTab instanceId="computer-b" />);

    expect(screen.queryByText('stale-market')).not.toBeInTheDocument();
    expect(screen.queryByText('stale-plugin')).not.toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Installed (0)' })).toBeInTheDocument();
    expect(mockedInvoke).toHaveBeenCalledWith('get_marketplace_governance', { instanceId: 'computer-b' });
  });
  it('installs without enabling, then enables through a separate explicit action', async () => {
    let installed = false;
    let enabled = false;
    mockedInvoke.mockImplementation(async (command) => {
      if (command === 'get_marketplace_governance') return {
        capabilities: supportedCapabilities,
        marketplaces: [{ name: 'acme', source: { type: 'remoteGit', displayGitUrl: null }, status: 'known' }],
        plugins: [{ marketplace: 'acme', plugin: 'new-tools', pluginId: 'new@acme', installed, enabled,
          status: !installed ? 'available' : enabled ? 'enabled' : 'disabled', bundledSkills: [], bundledMcpServers: [], declared: null }],
      };
      if (command === 'list_skills') return [];
      if (command === 'install_plugin') { installed = true; return; }
      if (command === 'enable_plugin') { enabled = true; return; }
      throw new Error(`Unexpected command: ${command}`);
    });
    render(<MarketplaceTab instanceId="computer-a" />);
    fireEvent.click(await screen.findByRole('tab', { name: 'Discover plugins' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Install Plugin' }));
    const enable = await screen.findByRole('button', { name: 'Enable Plugin' });
    expect(mockedInvoke).toHaveBeenCalledWith('install_plugin', { instanceId: 'computer-a', request: { marketplace: 'acme', plugin: 'new-tools' } });
    expect(mockedInvoke).not.toHaveBeenCalledWith('enable_plugin', expect.anything());
    fireEvent.click(enable);
    expect(await screen.findByRole('button', { name: 'Disable Plugin' })).toBeInTheDocument();
    expect(screen.getByText(/Actual skill availability and MCP tool health/)).toBeInTheDocument();
  });

  it('keeps uninstall in More and requires confirmation before removing a package', async () => {
    mockedInvoke.mockImplementation(async (command) => {
      if (command === 'get_marketplace_governance') return {
        capabilities: supportedCapabilities, marketplaces: [],
        plugins: [{ marketplace: 'acme', plugin: 'tools', pluginId: 'tools@acme', installed: true, enabled: true,
          status: 'enabled', bundledSkills: [], bundledMcpServers: [], declared: null }],
      };
      return [];
    });
    render(<MarketplaceTab instanceId="computer-a" />);
    fireEvent.click(await screen.findByRole('button', { name: 'More' }));
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Uninstall Plugin' }));
    const confirmation = within(await screen.findByRole('dialog'));
    expect(mockedInvoke).not.toHaveBeenCalledWith('uninstall_plugin', expect.anything());
    fireEvent.click(confirmation.getByRole('button', { name: 'Uninstall Plugin' }));
    await waitFor(() => expect(mockedInvoke).toHaveBeenCalledWith('uninstall_plugin', { instanceId: 'computer-a', request: { marketplace: 'acme', plugin: 'tools' } }));
  });

  it('identifies the failed action and retries it without misreporting installation failure', async () => {
    let attempts = 0;
    mockedInvoke.mockImplementation(async (command) => {
      if (command === 'get_marketplace_governance') return {
        capabilities: supportedCapabilities, marketplaces: [],
        plugins: [{ marketplace: 'acme', plugin: 'tools', pluginId: 'tools@acme', installed: true, enabled: attempts > 1,
          status: 'disabled', bundledSkills: [], bundledMcpServers: [], declared: null }],
      };
      if (command === 'enable_plugin') { attempts += 1; if (attempts === 1) throw new Error('input unavailable'); }
      return [];
    });
    render(<MarketplaceTab instanceId="computer-a" />);
    fireEvent.click(await screen.findByRole('button', { name: 'Enable Plugin' }));
    expect(await screen.findByText('Enable operation failed')).toBeInTheDocument();
    expect(screen.getByText('input unavailable')).toBeInTheDocument();
    expect(screen.queryByText('Installation failed')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Retry operation' }));
    expect(await screen.findByRole('button', { name: 'Disable Plugin' })).toBeInTheDocument();
    expect(attempts).toBe(2);
  });

  it('searches installed plugins across sources and separates source sync from reloading the list', async () => {
    mockedInvoke.mockImplementation(async (command) => {
      if (command === 'get_marketplace_governance') return {
        capabilities: supportedCapabilities,
        marketplaces: ['first', 'second'].map((name) => ({ name, source: { type: 'remoteGit', displayGitUrl: null }, status: 'known' })),
        plugins: ['first', 'second'].map((marketplace) => ({ marketplace, plugin: marketplace + '-tools', pluginId: marketplace,
          installed: true, enabled: false, status: 'disabled', bundledSkills: [], bundledMcpServers: [], declared: null })),
      };
      return [];
    });
    render(<MarketplaceTab instanceId="computer-a" />);
    expect(await screen.findByRole('button', { name: 'Select plugin second-tools (second)' })).toBeInTheDocument();
    fireEvent.change(screen.getByRole('textbox', { name: 'Search plugins by name, purpose or source' }), { target: { value: 'second' } });
    expect(screen.queryByRole('button', { name: 'Select plugin first-tools (first)' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Select plugin second-tools (second)' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Manage sources' }));
    expect(await screen.findByText(/Syncing a source refreshes its catalog/)).toBeInTheDocument();
    expect(mockedInvoke).not.toHaveBeenCalledWith('refresh_marketplace', expect.anything());
  });

  it('explains which library is managed while a saved directory switch awaits adoption', async () => {
    mockedInvoke.mockResolvedValue({ capabilities: supportedCapabilities, marketplaces: [], plugins: [] });
    useConfigAdoptionStore.setState({ statuses: { 'computer-a': {
      incarnation: 1, generation: 1, state: 'pending', unknown_resources: [], changes: [{
        resource_id: 'skill_home', reason: 'skill_home', section: 'skills', action: 'restart_computer',
        current_value: '/old/home', saved_value: '/new/home',
      }],
    } } });
    const openRuntime = vi.fn();
    render(<MarketplaceTab instanceId="computer-a" onOpenRuntime={openRuntime} />);
    expect(await screen.findByText('Storage change not yet adopted')).toBeInTheDocument();
    expect(screen.getByText(/Plugin management still uses the current instance directory/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Open runtime workbench' }));
    expect(openRuntime).toHaveBeenCalledOnce();
  });

});
