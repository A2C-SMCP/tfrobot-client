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
});
