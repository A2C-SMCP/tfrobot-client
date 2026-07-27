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
        { name: 'tf-market', displayGitUrl: 'https://example.com/tf.git', status: 'known', message: 'lastUpdated=2026-07-03T06:08:14Z' },
        { name: 'acme', displayGitUrl: 'https://example.com/acme.git', status: 'known', message: null },
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
          version: null,
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

  it('preserves unknown, empty, declared, and installed capability semantics', async () => {
    mockedInvoke.mockResolvedValueOnce({
      capabilities: supportedCapabilities,
      marketplaces: [
        { name: 'tf-market', displayGitUrl: 'https://example.com/tf.git', status: 'known', message: null },
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
          { name: 'tf-mkt', displayGitUrl: 'https://example.com/private.git', status: 'known', message: null },
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
          gitUrl: 'https://oauth2:test-token@example.com/private.git?ref=release#v1',
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
          { name: 'tf-market', displayGitUrl: 'https://example.com/tf.git', status: 'known', message: null },
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
        { name: 'tf-market', displayGitUrl: 'https://example.com/tf.git', status: 'known', message: null },
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

  it('previews an enabled plugin skill from the details pane', async () => {
    mockedInvoke
      .mockResolvedValueOnce({
        capabilities: supportedCapabilities,
        marketplaces: [
          { name: 'tf-market', displayGitUrl: 'https://example.com/tf.git', status: 'known', message: null },
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

  it('renders skill preview errors with a readable alert message', async () => {
    mockedInvoke
      .mockResolvedValueOnce({
        capabilities: supportedCapabilities,
        marketplaces: [
          { name: 'tf-market', displayGitUrl: 'https://example.com/tf.git', status: 'known', message: null },
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
        { name: 'stale-market', displayGitUrl: 'https://example.com/stale.git', status: 'known', message: null },
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
              { name: 'stale-market', displayGitUrl: 'https://example.com/stale.git', status: 'known', message: null },
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
