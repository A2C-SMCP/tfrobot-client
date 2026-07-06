import { invoke } from '@tauri-apps/api/core';
import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { MarketplaceTab } from '@/components/Computer/MarketplaceTab';
import { useSkillStore } from '@/stores/skillStore';

const mockedInvoke = vi.mocked(invoke);

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
        { name: 'tf-market', gitUrl: 'https://example.com/tf.git', status: 'known', message: 'lastUpdated=2026-07-03T06:08:14Z' },
        { name: 'acme', gitUrl: 'https://example.com/acme.git', status: 'known', message: null },
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
        {
          marketplace: 'acme',
          plugin: 'audit',
          pluginId: 'plugin-2',
          version: null,
          enabled: true,
          status: 'enabled',
          bundledMcpServers: ['audit-mcp'],
          bundledSkills: ['audit:code-review'],
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
    expect(screen.getByText('Last updated: 2026-07-03T06:08:14Z')).toBeInTheDocument();
    expect(screen.getByText('Plugins in tf-market')).toBeInTheDocument();
    expect(screen.getByText('Plugin Contents')).toBeInTheDocument();
    expect(screen.getByText('desktop-tools')).toBeInTheDocument();
    expect(screen.queryByText('audit')).not.toBeInTheDocument();
    expect(await screen.findByText('browser')).toBeInTheDocument();
    expect(screen.getByText('summarizer')).toBeInTheDocument();

    fireEvent.click(screen.getByText('acme'));

    expect(screen.getByText('Plugins in acme')).toBeInTheDocument();
    expect(screen.getAllByText('audit').length).toBeGreaterThan(0);
    expect(screen.queryByText('desktop-tools')).not.toBeInTheDocument();
    expect(screen.getByText('audit-mcp')).toBeInTheDocument();
    expect(screen.getByText('audit:code-review')).toBeInTheDocument();
  });

  it('updates an existing marketplace URL from the marketplace form', async () => {
    mockedInvoke
      .mockResolvedValueOnce({
        capabilities: supportedCapabilities,
        marketplaces: [
          { name: 'tf-mkt', gitUrl: 'https://example.com/old.git', status: 'known', message: null },
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
    fireEvent.change(screen.getByLabelText('Git URL'), { target: { value: 'https://example.com/new.git' } });
    fireEvent.click(screen.getByText('Update').closest('button')!);

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('update_marketplace', {
        instanceId: 'computer-a',
        request: {
          name: 'tf-mkt',
          gitUrl: 'https://example.com/new.git',
        },
      });
    });
  });

  it('adds a marketplace directly without secondary trust confirmation', async () => {
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
        marketplaces: [
          { name: 'tf-market', gitUrl: 'https://example.com/tf.git', status: 'known', message: null },
        ],
        plugins: [],
      })
      .mockResolvedValueOnce([]);

    render(<MarketplaceTab instanceId="computer-a" />);

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
    }).mockResolvedValueOnce([]);

    render(<MarketplaceTab instanceId="computer-a" />);

    expect(await screen.findByText('desktop-tools')).toBeInTheDocument();
    expect(screen.getByText('Enable Plugin').closest('button')).toBeEnabled();
    expect(screen.queryByText('Refresh Marketplace')).not.toBeInTheDocument();
    expect(screen.getByLabelText('Remove Marketplace')).toBeDisabled();
    expect(screen.getByText('Add').closest('button')).toBeDisabled();
  });

  it('previews an enabled plugin skill from the details pane', async () => {
    mockedInvoke
      .mockResolvedValueOnce({
        capabilities: supportedCapabilities,
        marketplaces: [
          { name: 'tf-market', gitUrl: 'https://example.com/tf.git', status: 'known', message: null },
        ],
        plugins: [
          {
            marketplace: 'tf-market',
            plugin: 'desktop-tools',
            pluginId: 'plugin-1',
            version: '1.0.0',
            enabled: true,
            status: 'enabled',
            bundledMcpServers: ['browser'],
            bundledSkills: ['summarizer'],
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

  it('renders skill preview errors with a readable alert message', async () => {
    mockedInvoke
      .mockResolvedValueOnce({
        capabilities: supportedCapabilities,
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
            status: 'disabled',
            bundledMcpServers: [],
            bundledSkills: ['summarizer'],
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
            capabilities: supportedCapabilities,
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
    expect(screen.getByText('Select a marketplace to manage plugins')).toBeInTheDocument();
    expect(mockedInvoke).toHaveBeenCalledWith('get_marketplace_governance', { instanceId: 'computer-b' });
  });
});
