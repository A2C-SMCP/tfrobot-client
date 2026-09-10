import { NavigationMemoryProvider, NavigationScope } from '@/components/Navigation/NavigationMemory';
import { NavigationMemory } from '@/stores/navigationStore';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { StrictMode, useCallback, useState } from 'react';
import { act, fireEvent, render, screen, waitFor } from '../helpers/render';
import { MarketplaceTab } from '@/components/Computer/MarketplaceTab';
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
    mockedInvoke.mockReset();
    mockedOpen.mockReset();
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

    expect(await screen.findByText('No SDK marketplaces returned')).toBeInTheDocument();
    expect(screen.getByText('Select a marketplace to manage plugins')).toBeInTheDocument();
    expect(screen.getByText('Add').closest('button')).toBeDisabled();
    expect(screen.queryByText('SDK marketplace lifecycle is unavailable')).not.toBeInTheDocument();
    expect(screen.queryByText('Computer::install_plugin')).not.toBeInTheDocument();
    expect(screen.queryByText('Install scope')).not.toBeInTheDocument();
  });

  it('renders three-pane marketplace, filtered plugin list, and selected plugin contents', async () => {
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

    expect(await screen.findByText('Marketplaces')).toBeInTheDocument();
    expect(await screen.findByText('Last updated: 2026-07-03T06:08:14Z')).toBeInTheDocument();
    expect(screen.getByText('Plugins in tf-market')).toBeInTheDocument();
    expect(screen.getByText('Plugin Contents')).toBeInTheDocument();
    expect(screen.getAllByText('desktop-tools').length).toBeGreaterThan(0);
    expect(screen.queryByText('audit')).not.toBeInTheDocument();
    expect(await screen.findByText('browser')).toBeInTheDocument();
    expect(screen.getByText('summarizer')).toBeInTheDocument();

    const acmeSelection = screen.getByRole('button', {
      name: 'Select marketplace acme',
    });
    fireEvent.click(acmeSelection);

    expect(screen.getByText('Plugins in acme')).toBeInTheDocument();
    expect(screen.getAllByText('audit').length).toBeGreaterThan(0);
    expect(screen.queryByText('desktop-tools')).not.toBeInTheDocument();
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
    expect(await screen.findByText('Plugins in tf-market')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Open audit Plugin' }));

    expect(await screen.findByText('Plugins in acme')).toBeInTheDocument();
    expect(screen.getAllByText('audit').length).toBeGreaterThan(0);
    expect(screen.getByText('audit-mcp')).toBeInTheDocument();
    expect(screen.queryByText('legacy-audit-mcp')).not.toBeInTheDocument();
    expect(screen.queryByText('desktop-tools')).not.toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByTestId('plugin-navigation-state')).toHaveTextContent('idle');
    });

    fireEvent.click(screen.getByText('tf-market'));
    expect((await screen.findAllByText('desktop-tools')).length).toBeGreaterThan(0);

    fireEvent.click(screen.getByRole('button', { name: 'Open audit Plugin' }));
    expect(await screen.findByText('Plugins in acme')).toBeInTheDocument();
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

    expect(await screen.findByText('Bundled skill capabilities are unknown')).toBeInTheDocument();
    expect(screen.getByText('Bundled MCP server capabilities are unknown')).toBeInTheDocument();

    fireEvent.click(screen.getByText('empty-plugin'));
    expect(await screen.findByText('No bundled skills')).toBeInTheDocument();
    expect(screen.getByText('No bundled MCP servers')).toBeInTheDocument();

    fireEvent.click(screen.getByText('declared-plugin'));
    expect(await screen.findByText('catalog-skill')).toBeInTheDocument();
    expect(screen.getByText('catalog-mcp')).toBeInTheDocument();

    fireEvent.click(screen.getByText('installed-plugin'));
    expect(await screen.findByText('actual-skill')).toBeInTheDocument();
    expect(screen.getByText('actual-mcp')).toBeInTheDocument();
    expect(screen.queryByText('declared-only-skill')).not.toBeInTheDocument();
    expect(screen.queryByText('declared-only-mcp')).not.toBeInTheDocument();
  });

  it('updates an existing marketplace URL from the marketplace form', async () => {
    mockedInvoke
      .mockResolvedValueOnce({
        capabilities: supportedCapabilities,
        marketplaces: [
          { name: 'tf-mkt', source: { type: 'remoteGit', displayGitUrl: 'https://example.com/private.git' }, status: 'known', message: null },
        ],
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

    expect(await screen.findByText('tf-mkt')).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('Edit'));
    expect(screen.getByText('Update Marketplace')).toBeInTheDocument();
    expect(screen.getByLabelText('Git URL')).toHaveValue('');
    expect(screen.getByText(/enter the complete Git URL again/i)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('Git URL'), {
      target: { value: 'https://oauth2:test-token@example.com/private.git?ref=release#v1' },
    });
    fireEvent.click(screen.getByText('Update').closest('button')!);

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('update_marketplace', {
        instanceId: 'computer-a',
        request: {
          name: 'tf-mkt',
          source: {
            type: 'remoteGit',
            gitUrl: 'https://oauth2:test-token@example.com/private.git?ref=release#v1',
          },
        },
      });
    });
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

    expect(await screen.findByText('No SDK marketplaces returned')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Add').closest('button')!);
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'local-market' } });
    fireEvent.change(screen.getByLabelText('Git URL'), {
      target: { value: 'file:///tmp/local-market' },
    });
    const addButtons = screen.getAllByText('Add');
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

    expect(await screen.findByText('No SDK marketplaces returned')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Add').closest('button')!);
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

    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'local-market' } });
    const addButtons = screen.getAllByText('Add');
    fireEvent.click(addButtons[addButtons.length - 1].closest('button')!);

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('add_marketplace', {
        instanceId: 'computer-a',
        request: {
          name: 'local-market',
          source: { type: 'localGit', path: '/tmp/Marketplace 空格' },
        },
      });
    });
  });

  it('recognizes a saved local source and blocks source changes while Plugins are installed', async () => {
    mockedInvoke
      .mockResolvedValueOnce({
        capabilities: supportedCapabilities,
        marketplaces: [{
          name: 'local-market',
          source: { type: 'localGit', path: '/tmp/local-market' },
          status: 'known',
          message: null,
        }],
        plugins: [{
          marketplace: 'local-market',
          plugin: 'tools',
          pluginId: 'tools@local-market',
          version: null,
          installed: true,
          enabled: false,
          status: 'disabled',
          bundledMcpServers: [],
          bundledSkills: [],
          declared: null,
          message: null,
        }],
      })
      .mockResolvedValueOnce([]);

    render(<MarketplaceTab instanceId="computer-a" />);

    expect(await screen.findByText('/tmp/local-market')).toBeInTheDocument();
    expect(screen.getByText('Local repository')).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('Edit'));

    expect(screen.getByLabelText('Local repository')).toBeChecked();
    expect(screen.getByLabelText('Local repository path')).toHaveValue('/tmp/local-market');
    expect(screen.getByText(/uninstall them before changing its source/i)).toBeInTheDocument();
    expect(screen.getByText('Update').closest('button')).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Choose folder' })).toBeDisabled();
  });

  it('updates an existing local Marketplace from a newly selected directory', async () => {
    mockedOpen.mockResolvedValueOnce('/tmp/local-market-next');
    mockedInvoke
      .mockResolvedValueOnce({
        capabilities: supportedCapabilities,
        marketplaces: [{
          name: 'local-market',
          source: { type: 'localGit', path: '/tmp/local-market' },
          status: 'known',
          message: null,
        }],
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

    expect(await screen.findByText('/tmp/local-market')).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('Edit'));
    fireEvent.click(screen.getByRole('button', { name: 'Choose folder' }));
    await waitFor(() => {
      expect(screen.getByLabelText('Local repository path')).toHaveValue('/tmp/local-market-next');
    });
    fireEvent.click(screen.getByText('Update').closest('button')!);

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('update_marketplace', {
        instanceId: 'computer-a',
        request: {
          name: 'local-market',
          source: { type: 'localGit', path: '/tmp/local-market-next' },
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

    const refresh = await screen.findByRole('button', { name: 'Refresh Marketplace' });
    fireEvent.click(refresh);

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('refresh_marketplace', {
        instanceId: 'computer-a',
        marketplace: 'tf-market',
      });
    });
    expect(await screen.findByText('Marketplace refreshed')).toBeInTheDocument();
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

    fireEvent.click(await screen.findByRole('button', { name: 'Refresh Marketplace' }));

    expect(await screen.findByText('refresh failed')).toBeInTheDocument();
    expect(screen.getByRole('button', {
      name: 'Select marketplace tf-market',
    })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
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

    expect(await screen.findByText('No SDK marketplaces returned')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Add').closest('button')!);
    expect(screen.getByText('Add Marketplace')).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'tf-market' } });
    fireEvent.change(screen.getByLabelText('Git URL'), { target: { value: 'https://example.com/tf.git' } });
    const addButtons = screen.getAllByText('Add');
    fireEvent.click(addButtons[addButtons.length - 1].closest('button')!);

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('add_marketplace', {
        instanceId: 'computer-a',
        request: {
          name: 'tf-market',
          source: { type: 'remoteGit', gitUrl: 'https://example.com/tf.git' },
        },
      });
    });
    await waitFor(() => {
      expect(screen.getByText('Add Marketplace').closest('.ant-modal'))
        .toHaveClass('ant-zoom-leave');
    });
    expect(screen.getByText('Cloning Marketplace tf-market…')).toBeInTheDocument();
    expect(screen.queryByText(/trust/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/confirm/i)).not.toBeInTheDocument();

    await act(async () => {
      await useSkillStore.getState().fetchMarketplaceGovernance('computer-a');
    });
    expect(screen.getByText('Refresh').closest('button')).toBeDisabled();
    expect(screen.getAllByText('Add')[0].closest('button')).toBeDisabled();

    addMarketplace.resolve();

    expect(await screen.findByText('Marketplace added')).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.queryByText('Cloning Marketplace tf-market…')).not.toBeInTheDocument();
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

    expect(await screen.findByText('No SDK marketplaces returned')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Add').closest('button')!);
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'tf-market' } });
    fireEvent.change(screen.getByLabelText('Git URL'), { target: { value: 'https://example.com/tf.git' } });
    const addButtons = screen.getAllByText('Add');
    fireEvent.click(addButtons[addButtons.length - 1].closest('button')!);
    expect(await screen.findByText('Cloning Marketplace tf-market…')).toBeInTheDocument();

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

    expect(screen.queryByText('Marketplace added')).not.toBeInTheDocument();
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
    expect(screen.getByLabelText('Remove Marketplace')).toBeDisabled();
    expect(screen.getByText('Add').closest('button')).toBeDisabled();
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
      expect(screen.getAllByText('enabled').length).toBeGreaterThan(0);
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

    fireEvent.click(await screen.findByText('summarizer'));

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
    let refreshing = false;
    mockedInvoke.mockImplementation(async (command) => {
      if (command === 'get_marketplace_governance') return refreshing ? new Promise((resolve) => { finish = resolve; }) : governance;
      if (command === 'list_skills') return [];
      throw new Error(`Unexpected command: ${command}`);
    });
    const view = render(<MarketplaceTab instanceId="computer-a" />);
    await screen.findByText('No SDK marketplaces returned');
    const count = mockedInvoke.mock.calls.filter(([command]) => command === 'list_skills').length;
    refreshing = true;
    fireEvent.click(screen.getByRole('button', { name: /Refresh/ }));
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
    fireEvent.click(await screen.findByText('summarizer'));
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
            enabled: false,
            status: 'disabled',
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

    fireEvent.click(await screen.findByText('summarizer'));

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
    expect(screen.getByText('No SDK marketplaces returned')).toBeInTheDocument();
    expect(screen.getByText('Select a marketplace to manage plugins')).toBeInTheDocument();
    expect(mockedInvoke).toHaveBeenCalledWith('get_marketplace_governance', { instanceId: 'computer-b' });
  });
});
