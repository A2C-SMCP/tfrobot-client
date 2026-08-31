import { fireEvent, render, screen, waitFor, within } from '../helpers/render';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { ComputerSettings } from '@/components/ComputerSettings';
import { useComputerStore } from '@/stores/computerStore';
import { runtimeSnapshot } from '../helpers/store';

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

  it('saves MCP startup concurrency with the other general settings', async () => {
    mockInvoke.mockImplementation(async (command) => {
      if (command === 'list_computer_instances') return [computerResponse];
      if (command === 'rename_computer_instance') {
        return { ...computerResponse, mcp_start_concurrency: 9 };
      }
      return null;
    });
    render(<ComputerSettings />);

    const concurrency = await screen.findByRole('spinbutton', {
      name: 'MCP startup concurrency',
    });
    expect(concurrency).toHaveValue('7');
    fireEvent.change(concurrency, { target: { value: '9' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('rename_computer_instance', {
        request: {
          id: 'computer-a',
          name: 'Computer A',
          description: 'Primary Computer',
          mcpStartConcurrency: 9,
        },
      });
    });
    expect(screen.getByText('Applies the next time this Computer starts.')).toBeInTheDocument();
  });

  it('renders persistent configuration sections including Built-in Tools', async () => {
    render(<ComputerSettings />);

    expect(await screen.findByText('Computer A Settings')).toBeInTheDocument();
    const navigation = screen.getByLabelText('Computer settings sections');
    expect(navigation).toHaveClass('ant-menu-inline');
    expect(within(navigation).getByText('General')).toBeInTheDocument();
    expect(within(navigation).getByText('Skills')).toBeInTheDocument();
    expect(within(navigation).getByText('Plugins & Marketplace')).toBeInTheDocument();
    expect(within(navigation).getByText('MCP Servers')).toBeInTheDocument();
    expect(within(navigation).getByText('Inputs')).toBeInTheDocument();
    expect(within(navigation).getByText('Connection Policy')).toBeInTheDocument();
    expect(within(navigation).getByText('Built-in Tools')).toBeInTheDocument();

    fireEvent.click(within(navigation).getByText('Skills'));
    expect(screen.getByTestId('skills-settings')).toHaveTextContent('computer-a');
    fireEvent.click(within(navigation).getByText('Plugins & Marketplace'));
    expect(screen.getByTestId('plugins-settings')).toHaveTextContent('computer-a');
    fireEvent.click(within(navigation).getByText('MCP Servers'));
    expect(screen.getByTestId('mcp-settings')).toHaveTextContent('computer-a');
    fireEvent.click(within(navigation).getByText('Inputs'));
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
    fireEvent.click(screen.getByRole('button', { name: 'Back to Computer' }));
    expect(onNavigate).toHaveBeenCalledWith('computer-detail:runtime');

    fireEvent.click(
      within(screen.getByLabelText('Computer settings sections')).getByText('Inputs'),
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
});
