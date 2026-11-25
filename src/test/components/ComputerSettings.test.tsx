import { act, fireEvent, render, screen, waitFor, within } from '../helpers/render';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { ComputerSettings } from '@/components/ComputerSettings';
import { useComputerStore } from '@/stores/computerStore';
import { runtimeSnapshot } from '../helpers/store';
import { NavigationMemoryProvider } from '@/components/Navigation/NavigationMemory';
import { NavigationMemory } from '@/stores/navigationStore';

const mockInvoke = vi.mocked(invoke);

vi.mock('@/components/Computer/MarketplaceTab', () => ({
  MarketplaceTab: ({
    instanceId,
    targetPlugin,
  }: {
    instanceId: string;
    targetPlugin?: { marketplace: string; plugin: string } | null;
  }) => (
    <div data-testid="plugins-settings">
      Plugins:{instanceId}:{targetPlugin?.marketplace}/{targetPlugin?.plugin}
    </div>
  ),
}));

vi.mock('@/components/McpConfig', () => ({
  McpConfig: ({
    instanceId,
    onOpenPlugin,
  }: {
    instanceId: string;
    onOpenPlugin?: (owner: {
      type: 'plugin';
      marketplace: string;
      plugin: string;
      pluginId: string;
    }) => void;
  }) => (
    <div data-testid="mcp-settings">
      MCP:{instanceId}
      <button
        onClick={() => onOpenPlugin?.({
          type: 'plugin',
          marketplace: 'acme',
          plugin: 'audit',
          pluginId: 'plugin-2',
        })}
      >
        Manage owning Plugin
      </button>
    </div>
  ),
}));

vi.mock('@/components/InputVariables', () => ({
  InputVariables: ({ instanceId }: { instanceId: string }) => (
    <div data-testid="inputs-settings">Inputs:{instanceId}</div>
  ),
}));

vi.mock('@/components/RobotConnectionPanel', () => ({
  RobotConnectionPanel: ({ instanceId }: { instanceId: string }) => (
    <div data-testid="connection-settings">Connection:{instanceId}</div>
  ),
}));

vi.mock('@/components/ComputerSettings/SkillsSettings', () => ({
  SkillsSettings: ({ instance }: { instance: { id: string } }) => (
    <div data-testid="skills-settings">Skills:{instance.id}</div>
  ),
}));

const computerResponse = {
  id: 'computer-a',
  name: 'Computer A',
  description: 'Primary Computer',
  running: true,
  runtime: runtimeSnapshot({ lifecycle: 'started' }),
  connected: false,
  mcp_server_count: 1,
  robot_binding: null,
  connection_policy: { target: null, auto_connect: false },
  mcp_start_concurrency: 7,
  connection: null,
};

describe('ComputerSettings', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useComputerStore.getState().reset();
    mockInvoke.mockImplementation(async (command) => {
      if (command === 'list_computer_instances') return [computerResponse];
      return null;
    });
  });

  it('routes built-in tool failures to the existing runtime workbench', async () => {
    mockInvoke.mockImplementation(async (command) => {
      if (command === 'list_computer_instances') return [computerResponse];
      if (command === 'get_client_control_catalog') return [];
      if (command === 'get_remote_control_policy') return {
        enabled: false, tool_scope: { mode: 'all' }, target_scope: { mode: 'self_only' },
      };
      if (command === 'get_command_line_tool_state') return {
        policy: { enabled: true }, effectiveWorkspace: '/workspace',
        runtimeState: 'error', assetsAvailable: true, error: 'connection failed',
      };
      return null;
    });
    const onNavigate = vi.fn();
    render(<ComputerSettings initialSection="built-in-tools" onNavigate={onNavigate} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Runtime workbench' }));
    expect(onNavigate).toHaveBeenCalledWith('computer-detail:runtime');
  });

  it('automatically saves only the completed concurrency edit', async () => {
    mockInvoke.mockImplementation(async (command) => {
      if (command === 'list_computer_instances') return [computerResponse];
      if (command === 'patch_computer_general') {
        return { ...computerResponse, mcp_start_concurrency: 9 };
      }
      return null;
    });
    render(<ComputerSettings initialSection="advanced" />);

    const concurrency = await screen.findByRole('spinbutton', {
      name: 'MCP startup concurrency',
    });
    expect(concurrency).toHaveValue('7');
    fireEvent.change(concurrency, { target: { value: '9' } });
    expect(screen.queryByRole('button', { name: 'Save' })).not.toBeInTheDocument();
    fireEvent.blur(concurrency);

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('patch_computer_general', {
        id: 'computer-a', patch: { field: 'mcpStartConcurrency', value: 9 },
      });
    });
    expect(screen.getByText(
      'Applies the next time this Computer starts; changing it does not interrupt the running Computer.',
    )).toBeInTheDocument();
  });

  it('renders persistent configuration sections including Built-in Tools', async () => {
    render(<ComputerSettings />);

    expect(await screen.findByText('Computer A Settings')).toBeInTheDocument();
    const navigation = screen.getByLabelText('Computer settings sections');
    expect(navigation).toHaveClass('ant-menu-inline');
    expect(within(navigation).getByText('General')).toBeInTheDocument();
    expect(within(navigation).getByText('Advanced')).toBeInTheDocument();
    expect(within(navigation).getByText('Plugins')).toBeInTheDocument();
    expect(within(navigation).getByText('MCP Servers')).toBeInTheDocument();
    expect(within(navigation).getByText('Variables and credentials')).toBeInTheDocument();
    expect(within(navigation).getByText('Connection Policy')).toBeInTheDocument();
    expect(within(navigation).getByText('Built-in Tools')).toBeInTheDocument();

    fireEvent.click(within(navigation).getByText('Advanced'));
    expect(screen.getByTestId('skills-settings')).toHaveTextContent('computer-a');
    fireEvent.click(within(navigation).getByText('Plugins'));
    expect(screen.getByTestId('plugins-settings')).toHaveTextContent('computer-a');
    fireEvent.click(within(navigation).getByText('MCP Servers'));
    expect(screen.getByTestId('mcp-settings')).toHaveTextContent('computer-a');
    fireEvent.click(within(navigation).getByText('Variables and credentials'));
    expect(screen.getByTestId('inputs-settings')).toHaveTextContent('computer-a');
    fireEvent.click(within(navigation).getByText('Connection Policy'));
    expect(screen.getByTestId('connection-settings')).toHaveTextContent('computer-a');

    expect(screen.queryByRole('button', { name: 'Start' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Stop' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Restart' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Connect' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Disconnect' })).not.toBeInTheDocument();
  });

  it('returns to the Computer runtime and keeps section navigation addressable', async () => {
    const onNavigate = vi.fn();
    render(
      <ComputerSettings
        initialSection="mcp"
        onNavigate={onNavigate}
      />,
    );

    expect(await screen.findByTestId('mcp-settings')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Back to runtime workbench' }));
    expect(onNavigate).toHaveBeenCalledWith('computer-detail:top');

    fireEvent.click(
      within(screen.getByLabelText('Computer settings sections')).getByText('Variables and credentials'),
    );
    expect(onNavigate).toHaveBeenCalledWith('computer-settings:inputs');
  });

  it('opens the exact owning Plugin from MCP settings', async () => {
    const onNavigate = vi.fn();
    render(
      <ComputerSettings
        initialSection="mcp"
        onNavigate={onNavigate}
      />,
    );

    fireEvent.click(await screen.findByRole('button', { name: 'Manage owning Plugin' }));
    await waitFor(() => {
      expect(screen.getByTestId('plugins-settings')).toHaveTextContent(
        'computer-a:acme/audit',
      );
    });
    expect(onNavigate).toHaveBeenCalledWith(
      'computer-settings:plugins:acme:audit:plugin-2',
    );
  });

  it('restores a targeted Plugin destination', async () => {
    render(
      <ComputerSettings
        initialSection="plugins"
        targetPlugin={{
          type: 'plugin',
          marketplace: 'acme',
          plugin: 'audit',
          pluginId: 'plugin-2',
        }}
      />,
    );

    expect(await screen.findByTestId('plugins-settings')).toHaveTextContent(
      'computer-a:acme/audit',
    );
  });
  it('preserves the latest A edit across queued A then B saves', async () => {
    const releases: Array<(value: unknown) => void> = [];
    mockInvoke.mockImplementation(async (command) => {
      if (command === 'list_computer_instances') return [computerResponse];
      if (command === 'patch_computer_general') return new Promise((resolve) => releases.push(resolve));
      return null;
    });
    render(<ComputerSettings />);
    const field = await screen.findByRole('textbox', { name: 'Name' });
    fireEvent.change(field, { target: { value: 'A' } });
    fireEvent.blur(field);
    await waitFor(() => expect(releases).toHaveLength(1));
    fireEvent.change(field, { target: { value: 'B' } });
    fireEvent.blur(field);
    await waitFor(() => expect(screen.getByText('Saving…')).toBeInTheDocument());
    fireEvent.change(field, { target: { value: 'A' } });
    await act(async () => releases[0]({ ...computerResponse, name: 'A' }));
    await waitFor(() => expect(releases).toHaveLength(2));
    await act(async () => releases[1]({ ...computerResponse, name: 'B' }));
    await waitFor(() => expect(useComputerStore.getState().instances[0].name).toBe('B'));
    expect(field).toHaveValue('A');
    expect(screen.getByText('Unsaved changes')).toBeInTheDocument();
  }, 15000);

  it.each(['success', 'failure', 'newer name', 'newer description', 'pending return'] as const)('preserves per-Computer save results across unmount with %s', async (outcome) => {
    let finish!: () => void;
    let first = true;
    let savedA = { ...computerResponse };
    const savedB = { ...computerResponse, id: 'computer-b', name: 'Computer B' };
    mockInvoke.mockImplementation(async (command, args) => {
      if (command === 'list_computer_instances') return [savedA, savedB];
      if (command === 'patch_computer_general') {
        const { id, patch } = args as { id: string; patch: { field: string; value: string } };
        expect(id).toBe('computer-a');
        if (first) {
          first = false;
          return new Promise((resolve, reject) => { finish = () => {
            if (outcome === 'failure') reject(new Error('save unavailable'));
            else { savedA = { ...savedA, name: patch.value }; resolve(savedA); }
          }; });
        }
        savedA = { ...savedA, name: patch.value };
        return savedA;
      }
      return null;
    });
    const memory = new NavigationMemory();
    render(<NavigationMemoryProvider memory={memory}><ComputerSettings /></NavigationMemoryProvider>);
    fireEvent.change(await screen.findByRole('textbox', { name: 'Name' }), { target: { value: 'Submitted A' } });
    fireEvent.blur(screen.getByRole('textbox', { name: 'Name' }));
    await waitFor(() => expect(finish).toBeDefined());
    act(() => useComputerStore.getState().selectInstance('computer-b'));
    await waitFor(() => expect(screen.getByRole('textbox', { name: 'Name' })).toHaveValue('Computer B'));
    if (outcome.startsWith('newer') || outcome === 'pending return') {
      act(() => useComputerStore.getState().selectInstance('computer-a'));
      const field = await screen.findByRole('textbox', { name: outcome === 'newer name' ? 'Name' : 'Description' });
      if (outcome !== 'pending return') fireEvent.change(field, { target: { value: 'Newer draft' } });
    }
    await act(async () => { finish(); });
    if (!outcome.startsWith('newer') && outcome !== 'pending return') {
      expect(screen.getByRole('textbox', { name: 'Name' })).toHaveValue('Computer B');
      act(() => useComputerStore.getState().selectInstance('computer-a'));
    }
    const name = await screen.findByRole('textbox', { name: 'Name' });
    const nameStatus = within(name.closest('.ant-form-item')!).getByRole('status');
    expect(name).toHaveValue(outcome === 'newer name' ? 'Newer draft' : 'Submitted A');
    expect(nameStatus).toHaveTextContent(outcome === 'failure' ? 'Save failed' : outcome === 'newer name' ? 'Unsaved changes' : /^Saved$/);
    if (outcome === 'newer description') {
      const description = screen.getByRole('textbox', { name: 'Description' });
      expect(description).toHaveValue('Newer draft');
      expect(within(description.closest('.ant-form-item')!).getByRole('status')).toHaveTextContent('Unsaved changes');
    }
    if (outcome === 'failure') {
      fireEvent.click(within(name.closest('.ant-form-item')!).getByRole('button', { name: 'Retry saving' }));
      await waitFor(() => expect(nameStatus).toHaveTextContent(/^Saved$/));
    }
  }, 15_000);

});
