import { invoke } from '@tauri-apps/api/core';
import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { MarketplaceTab } from '@/components/Computer/MarketplaceTab';
import { useSkillStore } from '@/stores/skillStore';

const mockedInvoke = vi.mocked(invoke);

describe('MarketplaceTab', () => {
  beforeEach(() => {
    useSkillStore.getState().reset();
    mockedInvoke.mockReset();
  });

  it('renders unsupported SDK governance state without enabling lifecycle actions', async () => {
    mockedInvoke.mockResolvedValueOnce({
      capabilities: {
        computerLifecycleApiAvailable: false,
        supportedOperations: [],
        requiredSdkApis: ['Computer::install_plugin'],
        reason: 'SDK lifecycle unavailable',
      },
      marketplaces: [],
      plugins: [],
    });

    render(<MarketplaceTab instanceId="computer-a" />);

    expect(await screen.findByText('SDK marketplace lifecycle is unavailable')).toBeInTheDocument();
    expect(screen.getByText('Computer::install_plugin')).toBeInTheDocument();
    expect(screen.getByText('No SDK marketplaces returned')).toBeInTheDocument();
    expect(screen.getByText('No SDK plugins returned')).toBeInTheDocument();
    expect(screen.getByText('Add').closest('button')).toBeDisabled();
  });

  it('renders SDK governance marketplace and plugin lists', async () => {
    mockedInvoke.mockResolvedValueOnce({
      capabilities: {
        computerLifecycleApiAvailable: true,
        supportedOperations: ['refresh_marketplace', 'enable_plugin'],
        requiredSdkApis: [],
        reason: 'supported',
      },
      marketplaces: [
        { name: 'tf-market', gitUrl: 'https://example.com/tf.git', status: 'known', message: null },
      ],
      plugins: [
        {
          marketplace: 'tf-market',
          plugin: 'desktop-tools',
          pluginId: 'plugin-1',
          version: '1.0.0',
          enabled: false,
          status: 'installed',
          bundledMcpServers: ['browser'],
          bundledSkills: ['summarizer'],
          message: null,
        },
      ],
    });
    mockedInvoke.mockResolvedValueOnce(undefined);
    mockedInvoke.mockResolvedValueOnce({
      capabilities: {
        computerLifecycleApiAvailable: true,
        supportedOperations: ['refresh_marketplace', 'enable_plugin'],
        requiredSdkApis: [],
        reason: 'supported',
      },
      marketplaces: [],
      plugins: [],
    });
    mockedInvoke.mockResolvedValueOnce([]);

    render(<MarketplaceTab instanceId="computer-a" />);

    expect((await screen.findAllByText('tf-market')).length).toBeGreaterThan(0);
    expect(screen.getByText('desktop-tools')).toBeInTheDocument();
    expect(screen.getByText('Bundled skills: summarizer')).toBeInTheDocument();
    expect(screen.getByText('Bundled MCP servers: browser')).toBeInTheDocument();

    fireEvent.click(screen.getByText('Enable Plugin').closest('button')!);

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('enable_plugin', {
        instanceId: 'computer-a',
        request: {
          marketplace: 'tf-market',
          plugin: 'desktop-tools',
          pluginId: 'plugin-1',
          version: '1.0.0',
          enabled: false,
          status: 'installed',
          bundledMcpServers: ['browser'],
          bundledSkills: ['summarizer'],
          message: null,
        },
      });
    });
  }, 10000);

  it('adds a marketplace directly without secondary trust confirmation', async () => {
    mockedInvoke
      .mockResolvedValueOnce({
        capabilities: {
          computerLifecycleApiAvailable: true,
          supportedOperations: ['add_marketplace'],
          requiredSdkApis: [],
          reason: 'supported',
        },
        marketplaces: [],
        plugins: [],
      })
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce({
        capabilities: {
          computerLifecycleApiAvailable: true,
          supportedOperations: ['add_marketplace'],
          requiredSdkApis: [],
          reason: 'supported',
        },
        marketplaces: [
          { name: 'tf-market', gitUrl: 'https://example.com/tf.git', status: 'known', message: null },
        ],
        plugins: [],
      })
      .mockResolvedValueOnce([]);

    render(<MarketplaceTab instanceId="computer-a" />);

    expect(await screen.findByText('SDK marketplace lifecycle is available')).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'tf-market' } });
    fireEvent.change(screen.getByLabelText('Git URL'), { target: { value: 'https://example.com/tf.git' } });
    fireEvent.click(screen.getByText('Add').closest('button')!);

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('add_marketplace', {
        instanceId: 'computer-a',
        request: {
          name: 'tf-market',
          gitUrl: 'https://example.com/tf.git',
        },
      });
    });
    expect(screen.queryByText(/trust/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/confirm/i)).not.toBeInTheDocument();
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
        { name: 'tf-market', gitUrl: 'https://example.com/tf.git', status: 'known', message: null },
      ],
      plugins: [
        {
          marketplace: 'tf-market',
          plugin: 'desktop-tools',
          pluginId: 'plugin-1',
          version: '1.0.0',
          enabled: false,
          status: 'installed',
          bundledMcpServers: [],
          bundledSkills: [],
          message: null,
        },
      ],
    });

    render(<MarketplaceTab instanceId="computer-a" />);

    expect((await screen.findAllByText('tf-market')).length).toBeGreaterThan(0);
    expect(screen.getByText('Enable Plugin').closest('button')).toBeEnabled();
    expect(screen.getByText('Refresh Marketplace').closest('button')).toBeDisabled();
    expect(screen.getByText('Remove Marketplace').closest('button')).toBeDisabled();
    expect(screen.getByText('Add').closest('button')).toBeDisabled();
    expect(screen.getByText('Install Plugin').closest('button')).toBeDisabled();
  });

  it('fixes plugin install to the current Computer without exposing SDK scope selection', async () => {
    mockedInvoke
      .mockResolvedValueOnce({
        capabilities: {
          computerLifecycleApiAvailable: true,
          supportedOperations: ['install_plugin'],
          requiredSdkApis: [],
          reason: 'supported',
        },
        marketplaces: [],
        plugins: [],
      })
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce({
        capabilities: {
          computerLifecycleApiAvailable: true,
          supportedOperations: ['install_plugin'],
          requiredSdkApis: [],
          reason: 'supported',
        },
        marketplaces: [],
        plugins: [],
      })
      .mockResolvedValueOnce([]);

    render(<MarketplaceTab instanceId="computer-b" />);

    expect(await screen.findByText('computer-b')).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('Marketplace'), { target: { value: 'tf-market' } });
    fireEvent.change(screen.getByLabelText('Plugin'), { target: { value: 'desktop-tools' } });
    fireEvent.click(screen.getByText('Install Plugin').closest('button')!);

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('install_plugin', {
        instanceId: 'computer-b',
        request: {
          marketplace: 'tf-market',
          plugin: 'desktop-tools',
        },
      });
    });
    expect(screen.queryByLabelText(/scope/i)).not.toBeInTheDocument();
    expect(screen.queryByText('user')).not.toBeInTheDocument();
    expect(screen.queryByText('project')).not.toBeInTheDocument();
    expect(screen.queryByText('local')).not.toBeInTheDocument();
  });

  it('renders structured lifecycle errors with a readable message', async () => {
    mockedInvoke
      .mockResolvedValueOnce({
        capabilities: {
          computerLifecycleApiAvailable: true,
          supportedOperations: ['install_plugin'],
          requiredSdkApis: [],
          reason: 'supported',
        },
        marketplaces: [],
        plugins: [],
      })
      .mockRejectedValueOnce({
        message: "MCP server 'audit-mcp' already exists as a user-managed MCP server",
      });

    render(<MarketplaceTab instanceId="computer-a" />);

    expect(await screen.findByText('SDK marketplace lifecycle is available')).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('Marketplace'), { target: { value: 'acme' } });
    fireEvent.change(screen.getByLabelText('Plugin'), { target: { value: 'audit' } });
    fireEvent.click(screen.getByText('Install Plugin').closest('button')!);

    expect(await screen.findByText("MCP server 'audit-mcp' already exists as a user-managed MCP server")).toBeInTheDocument();
    expect(screen.queryByText('[object Object]')).not.toBeInTheDocument();
  });

  it('does not render stale marketplace governance from a different active instance', () => {
    mockedInvoke.mockReturnValue(new Promise(() => undefined) as any);
    useSkillStore.setState((state) => ({
      ...state,
      activeInstanceId: 'computer-a',
      capabilities: {
        computerLifecycleApiAvailable: true,
        supportedOperations: ['enable_plugin'],
        requiredSdkApis: [],
        reason: 'supported',
      },
      marketplaces: [
        { name: 'stale-market', gitUrl: 'https://example.com/stale.git', status: 'known', message: null },
      ],
      plugins: [
        {
          marketplace: 'stale-market',
          plugin: 'stale-plugin',
          pluginId: 'plugin-1',
          version: '1.0.0',
          enabled: false,
          status: 'installed',
          bundledMcpServers: [],
          bundledSkills: [],
          message: null,
        },
      ],
      recordsByInstanceId: {
        'computer-a': {
          skills: [],
          selectedSkillName: null,
          selectedSkill: null,
          governance: {
            capabilities: {
              computerLifecycleApiAvailable: true,
              supportedOperations: ['enable_plugin'],
              requiredSdkApis: [],
              reason: 'supported',
            },
            marketplaces: [
              { name: 'stale-market', gitUrl: 'https://example.com/stale.git', status: 'known', message: null },
            ],
            plugins: [
              {
                marketplace: 'stale-market',
                plugin: 'stale-plugin',
                pluginId: 'plugin-1',
                version: '1.0.0',
                enabled: false,
                status: 'installed',
                bundledMcpServers: [],
                bundledSkills: [],
                message: null,
              },
            ],
          },
          loadingSkills: false,
          loadingSkill: false,
          loadingMarketplace: false,
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
    expect(screen.getByText('No SDK plugins returned')).toBeInTheDocument();
    expect(mockedInvoke).toHaveBeenCalledWith('get_marketplace_governance', { instanceId: 'computer-b' });
  });
});
