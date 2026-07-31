import { render, screen, fireEvent, waitFor, within } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { Computer } from '@/components/Computer';
import { useComputerStore } from '@/stores/computerStore';
import { useInputStore } from '@/stores/inputStore';
import { useRuntimeStore } from '@/stores/runtimeStore';
import { runtimeSnapshot } from '../helpers/store';

const mockInvoke = vi.mocked(invoke);

vi.setConfig({ testTimeout: 60_000 });

const mockComputerInstances = [
  {
    id: 'computer-a',
    name: 'prod',
    description: 'Production computer',
    running: true,
    runtime: runtimeSnapshot({
      lifecycle: 'connected',
      mcp_servers: 5,
      active_mcp_servers: 5,
    }),
    connected: true,
    mcp_server_count: 5,
    robot_binding: {
      employee_id: 42,
      robot_id: 'robot-a',
      robot_account_id: 4200,
      namespace: 'test',
      robot_name: 'Robot A',
    },
    connection_policy: {
      target: { type: 'manager_robot', id: '42', robotAccountId: 4200 },
      auto_connect: false,
    },
    connection: {
      url: 'https://smcp.example.com',
      office_id: 'office-1',
      computer_name: 'prod',
      connected_at: '2026-01-01T00:00:00Z',
      profile_name: 'prod',
    },
  },
];

const twoComputerInstances = [
  {
    ...mockComputerInstances[0],
    id: 'computer-a',
    name: 'Computer A',
    mcp_server_count: 1,
  },
  {
    ...mockComputerInstances[0],
    id: 'computer-b',
    name: 'Second Computer',
    connected: false,
    runtime: runtimeSnapshot(),
    mcp_server_count: 2,
    connection: null,
  },
];

function last<T>(items: T[]): T {
  return items[items.length - 1];
}

vi.mock('@/components/DesktopResources', () => ({
  DesktopResources: ({ instanceId }: { instanceId: string }) => <div data-testid="desktop-resources">DesktopResources:{instanceId}</div>,
}));
vi.mock('@/components/DebugPanel', () => ({
  DebugPanel: ({ instanceId }: { instanceId: string }) => <div data-testid="debug-panel">DebugPanel:{instanceId}</div>,
}));
vi.mock('@/components/LogViewer', () => ({
  LogViewer: ({ instanceId }: { instanceId?: string }) => <div data-testid="log-viewer">LogViewer:{instanceId}</div>,
}));
vi.mock('@/components/Computer/ComputerRuntime', () => ({
  ComputerRuntime: ({
    instance,
    onOpenPlugin,
  }: {
    instance: { id: string };
    onOpenPlugin?: (owner: {
      type: 'plugin';
      marketplace: string;
      plugin: string;
      pluginId: string;
    }) => void;
  }) => (
    <div data-testid="computer-runtime">
      ComputerRuntime:{instance.id}
      <button
        type="button"
        onClick={() => onOpenPlugin?.({
          type: 'plugin',
          marketplace: 'acme',
          plugin: 'audit',
          pluginId: 'plugin-2',
        })}
      >
        Open Plugin from Runtime
      </button>
    </div>
  ),
}));
vi.mock('@/components/Computer/SkillsTab', () => ({
  SkillsTab: ({ instanceId }: { instanceId: string }) => <div data-testid="skills-tab">SkillsTab:{instanceId}</div>,
}));
const { mockFetchManualTargets } = vi.hoisted(() => ({
  mockFetchManualTargets: vi.fn(),
}));

vi.mock('@/stores/connectionTargetStore', () => ({
  useConnectionTargetStore: vi.fn(() => ({
    manualTargets: [
      {
        id: 'target-a',
        name: 'Target A',
        office_id: 'office-a',
      },
    ],
    fetchManualTargets: mockFetchManualTargets,
  })),
}));

describe('Computer', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockFetchManualTargets.mockReset();
    useRuntimeStore.getState().reset();
    useComputerStore.getState().reset();
    useInputStore.getState().reset();
    mockInvoke.mockResolvedValue(mockComputerInstances);
  });

  it('renders the Computer list', () => {
    mockInvoke.mockReturnValueOnce(new Promise(() => {}));
    useComputerStore.setState({
      instances: [
        {
          id: 'computer-a',
          name: 'prod',
          description: 'Production computer',
          status: 'running',
          connectionStatus: 'connected',
          connectionProfile: 'prod',
          robotName: 'Robot A',
          connectionPolicy: {
            target: { type: 'manager_robot', id: '42', robotAccountId: 4200 },
            auto_connect: false,
          },
          mcpServerCount: 5,
          runtime: runtimeSnapshot({
            lifecycle: 'connected',
            mcp_servers: 5,
            active_mcp_servers: 5,
          }),
        },
      ],
    });

    render(<Computer />);

    expect(screen.getByText('prod')).toBeInTheDocument();
    expect(screen.getByText('Production computer')).toBeInTheDocument();
    expect(screen.getByText('Bound robot: Robot A')).toBeInTheDocument();
    expect(screen.getByText('Connection profile: prod')).toBeInTheDocument();
    expect(screen.getByText('5 MCP servers')).toBeInTheDocument();
  });

  it('renders an empty state when the Computer list is empty', async () => {
    mockInvoke.mockResolvedValueOnce([]);

    render(<Computer />);

    expect(await screen.findByText('No Computer instances')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Open Details' })).not.toBeInTheDocument();
  });

  it('opens a Computer single-page runtime workbench from the list', async () => {
    render(<Computer />);

    fireEvent.click(await screen.findByRole('button', { name: 'prod' }));

    expect(screen.getByLabelText('Computer runtime workbench')).toBeInTheDocument();
    expect(screen.queryByText('Overview')).not.toBeInTheDocument();
    expect(screen.queryByRole('tab', { name: 'Runtime' })).not.toBeInTheDocument();
    expect(screen.getByLabelText('Desktop Resources')).toBeInTheDocument();
    expect(screen.getByText('Debug Panel')).toBeInTheDocument();
    expect(screen.getByText('Logs')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Open Computer settings' })).toBeInTheDocument();
    expect(screen.getByTestId('computer-runtime')).toHaveTextContent('computer-a');
    expect(screen.getByTestId('skills-tab')).toHaveTextContent('computer-a');
    expect(screen.getByTestId('desktop-resources')).toHaveTextContent('computer-a');
  }, 20000);

  it('opens the selected second Computer with scoped workbench sections', async () => {
    mockInvoke.mockResolvedValueOnce(twoComputerInstances);

    render(<Computer />);

    fireEvent.click(await screen.findByRole('button', { name: 'Second Computer' }));

    expect(screen.getByText('Second Computer')).toBeInTheDocument();
    expect(screen.getByTestId('computer-runtime')).toHaveTextContent('computer-b');
    expect(screen.getByTestId('skills-tab')).toHaveTextContent('computer-b');
    expect(screen.getByTestId('desktop-resources')).toHaveTextContent('computer-b');
  }, 20000);

  it('opens the runtime view directly', async () => {
    render(<Computer initialView="detail" initialSection="top" />);
    expect(await screen.findByTestId('computer-runtime')).toHaveTextContent('computer-a');
  }, 20000);

  it('routes Plugin-owned Runtime requests to the corresponding settings Plugin', async () => {
    const onNavigate = vi.fn();
    render(
      <Computer
        initialView="detail"
        initialSection="top"
        onNavigate={onNavigate}
      />,
    );

    fireEvent.click(await screen.findByRole('button', { name: 'Open Plugin from Runtime' }));

    expect(onNavigate).toHaveBeenCalledWith(
      'computer-settings:plugins:acme:audit:plugin-2',
    );
  }, 20000);

  it('opens settings from the detail gear', async () => {
    const onNavigate = vi.fn();
    render(<Computer initialView="detail" onNavigate={onNavigate} />);

    expect(await screen.findByText('prod')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Open Computer settings' }));
    expect(onNavigate).toHaveBeenCalledWith('computer-settings:general');
  });

  it('keeps ID copy next to the identity and only runtime or destructive actions in the more menu', async () => {
    render(<Computer initialView="detail" />);

    expect(await screen.findByText('prod')).toBeInTheDocument();
    const copyId = screen.getByRole('button', { name: 'Copy ID' });
    expect(copyId).toBeInTheDocument();
    fireEvent.click(copyId);
    expect(await screen.findByRole('button', { name: 'Computer ID copied' })).toBeInTheDocument();

    expect(screen.queryByRole('button', { name: 'Duplicate' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'More Computer actions' }));

    const menu = await screen.findByRole('menu');
    expect(within(menu).getByText('Restart')).toBeInTheDocument();
    expect(within(menu).getByText('Delete')).toBeInTheDocument();
    expect(within(menu).queryByText('View logs')).not.toBeInTheDocument();
    expect(within(menu).queryByText('Copy ID')).not.toBeInTheDocument();
    expect(within(menu).queryByText('Edit identity')).not.toBeInTheDocument();
  });

  it('maps the legacy debug destination to the expanded workbench section', async () => {
    render(<Computer initialView="detail" initialSection="debug" />);

    expect(await screen.findByText('prod')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Back to Computers' })).toBeInTheDocument();
    expect(await screen.findByTestId('debug-panel')).toHaveTextContent('computer-a');
  });

  it('can return from detail to list', async () => {
    render(<Computer initialView="detail" />);

    const back = await screen.findByRole('button', { name: 'Back to Computers' });
    expect(back).toHaveTextContent('');
    fireEvent.click(back);

    expect(screen.getByText('Create Computer')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'prod' })).toBeInTheDocument();
  }, 20000);

  it('disables list connect when the Computer is not running', async () => {
    const stoppedInstance = {
      id: 'computer-a',
      name: 'prod',
      running: false,
      runtime: runtimeSnapshot({ lifecycle: 'shutdown' }),
      connected: false,
      mcp_server_count: 0,
      robot_binding: null,
      connection_policy: { target: { type: 'manual_smcp', id: 'target-a' }, auto_connect: false },
      connection: null,
    };
    mockInvoke.mockResolvedValueOnce([stoppedInstance]);

    render(<Computer />);

    expect(await screen.findByRole('button', { name: 'Connect' })).toBeDisabled();
  }, 20000);

  it('disables list connect for a Manager Robot target without robotAccountId', async () => {
    const missingRobotAccountIdInstance = {
      ...mockComputerInstances[0],
      connected: false,
      runtime: runtimeSnapshot(),
      connection: null,
      connection_policy: { target: { type: 'manager_robot', id: '42' }, auto_connect: false },
    };
    mockInvoke.mockResolvedValueOnce([missingRobotAccountIdInstance]);

    render(<Computer />);

    expect(await screen.findByRole('button', { name: 'Connect' })).toBeDisabled();
  }, 20000);

  it('keeps Runtime action technical errors out of the ordinary Computer UI', async () => {
    const stoppedInstance = {
      ...mockComputerInstances[0],
      running: false,
      connected: false,
      runtime: runtimeSnapshot({ lifecycle: 'shutdown' }),
      connection: null,
    };
    mockInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'list_computer_instances') return [stoppedInstance];
      if (cmd === 'start_computer_instance') {
        throw new Error('spawn /secret/path failed with token=private');
      }
      return null;
    });

    render(<Computer />);
    fireEvent.click((await screen.findAllByRole('button', { name: 'Start' }))[0]);

    expect(await screen.findByText(
      'The Runtime operation failed. View Runtime diagnostics or logs for details.',
    )).toBeInTheDocument();
    expect(screen.queryByText(/secret\/path|token=private/)).not.toBeInTheDocument();
  }, 20000);

  it('connects the selected target from the list action', async () => {
    const disconnectedInstance = {
      ...mockComputerInstances[0],
      connected: false,
      runtime: runtimeSnapshot(),
      connection: null,
      connection_policy: { target: { type: 'manual_smcp', id: 'target-a' }, auto_connect: false },
    };
    mockInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'list_computer_instances') return [disconnectedInstance];
      if (cmd === 'connect_computer_connection_target') return null;
      return null;
    });
    useComputerStore.setState({
      instances: [
        {
          id: 'computer-a',
          name: 'prod',
          status: 'running',
          connectionStatus: 'disconnected',
          connectionPolicy: { target: { type: 'manual_smcp', id: 'target-a' }, auto_connect: false },
          mcpServerCount: 0,
          runtime: runtimeSnapshot(),
        },
      ],
    });

    render(<Computer />);

    fireEvent.click(await screen.findByRole('button', { name: 'Connect' }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('connect_computer_connection_target', {
        id: 'computer-a',
      });
    });
  }, 20000);

  it('keeps connection action technical errors out of the ordinary Computer UI', async () => {
    const disconnectedInstance = {
      ...mockComputerInstances[0],
      connected: false,
      runtime: runtimeSnapshot(),
      connection: null,
      connection_policy: { target: { type: 'manual_smcp', id: 'target-a' }, auto_connect: false },
    };
    mockInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'list_computer_instances') return [disconnectedInstance];
      if (cmd === 'connect_computer_connection_target') {
        throw new Error('https://private.example.test failed with token=private');
      }
      return null;
    });

    render(<Computer />);
    fireEvent.click(await screen.findByRole('button', { name: 'Connect' }));

    expect(await screen.findByText(
      'The connection operation failed. View Runtime diagnostics or logs for details.',
    )).toBeInTheDocument();
    expect(screen.queryByText(/private\.example|token=private/)).not.toBeInTheDocument();
  }, 20000);

  it('offers disconnect from list and detail when connection authority precedes joined-office', async () => {
    const authorityConnectedInstance = {
      ...mockComputerInstances[0],
      connected: false,
      client_connection_present: true,
      runtime: runtimeSnapshot({ lifecycle: 'connected' }),
      connection: null,
    };
    mockInvoke.mockResolvedValueOnce([authorityConnectedInstance]);

    render(<Computer />);

    expect(await screen.findByRole('button', { name: 'Disconnect' })).toBeEnabled();
    fireEvent.click(screen.getByRole('button', { name: 'prod' }));
    expect(screen.getByRole('button', { name: /Disconnect$/ })).toBeEnabled();
    expect(screen.queryByRole('button', { name: 'Connect' })).not.toBeInTheDocument();
  }, 20000);

  it('offers orphan transport cleanup from both list and detail capabilities', async () => {
    const orphanInstance = {
      ...mockComputerInstances[0],
      connected: false,
      connection: null,
      runtime: runtimeSnapshot({ lifecycle: 'connected' }),
      connection_state: {
        status: 'disconnected',
        present: false,
        revision: 8,
        context: null,
        operation: null,
        last_error: {
          operation: 'disconnect',
          message: 'Socket cleanup failed',
          retryable: true,
          occurred_at: '2026-07-29T02:00:00Z',
        },
        actions: {
          connect: { enabled: false, disabled_reason: 'connection_unavailable' },
          disconnect: { enabled: true, disabled_reason: null },
        },
      },
    };
    mockInvoke.mockResolvedValueOnce([orphanInstance]);

    render(<Computer />);

    expect(await screen.findByRole('button', { name: 'Disconnect' })).toBeEnabled();
    expect(screen.queryByRole('button', { name: 'Connect' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'prod' }));
    expect(screen.getByRole('button', { name: /Disconnect$/ })).toBeEnabled();
    expect(screen.queryByRole('button', { name: 'Connect' })).not.toBeInTheDocument();
  }, 20000);

  it('disables detail connect for a Manager Robot target without robotAccountId', async () => {
    const missingRobotAccountIdInstance = {
      ...mockComputerInstances[0],
      connected: false,
      runtime: runtimeSnapshot(),
      connection: null,
      connection_policy: { target: { type: 'manager_robot', id: '42' }, auto_connect: false },
    };
    mockInvoke.mockResolvedValueOnce([missingRobotAccountIdInstance]);

    render(<Computer initialView="detail" />);

    expect((await screen.findByText('Connect')).closest('button')).toBeDisabled();
  }, 20000);

  it('creates a Computer from the list page', async () => {
    mockInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'list_computer_instances') return mockComputerInstances;
      if (cmd === 'create_computer_instance') {
        return {
          id: 'computer-new',
          name: 'New Computer',
          description: 'New description',
          running: false,
          runtime: runtimeSnapshot({ lifecycle: 'shutdown' }),
          connected: false,
          mcp_server_count: 0,
          robot_binding: null,
          connection: null,
        };
      }
      return null;
    });

    render(<Computer />);

    fireEvent.click(await screen.findByText('Create Computer'));
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'New Computer' } });
    fireEvent.change(screen.getByLabelText('Description'), { target: { value: 'New description' } });
    fireEvent.click(last(screen.getAllByText('OK')));

    expect(await screen.findByText('New Computer')).toBeInTheDocument();
    expect(mockInvoke).toHaveBeenCalledWith('create_computer_instance', {
      request: { name: 'New Computer', description: 'New description' },
    });
  });

  it('edits, duplicates, stops, and starts from the list actions', async () => {
    mockInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'list_computer_instances') return mockComputerInstances;
      if (cmd === 'rename_computer_instance') return { ...mockComputerInstances[0], name: 'Updated Computer', description: 'Updated description' };
      if (cmd === 'duplicate_computer_instance') return {
        ...mockComputerInstances[0],
        id: 'computer-copy',
        name: 'prod Copy',
        running: false,
        runtime: runtimeSnapshot({ lifecycle: 'shutdown' }),
      };
      if (cmd === 'stop_computer_instance') return {
        ...mockComputerInstances[0],
        running: false,
        runtime: runtimeSnapshot({
          lifecycle: 'shutdown',
          generation: 1,
          snapshot_revision: 2,
        }),
      };
      if (cmd === 'start_computer_instance') return {
        ...mockComputerInstances[0],
        running: true,
        runtime: runtimeSnapshot({
          lifecycle: 'started',
          generation: 2,
          snapshot_revision: 3,
        }),
      };
      return null;
    });

    render(<Computer />);

    fireEvent.click((await screen.findAllByRole('button', { name: 'Edit' }))[0]);
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Updated Computer' } });
    fireEvent.change(screen.getByLabelText('Description'), { target: { value: 'Updated description' } });
    fireEvent.click(last(screen.getAllByText('OK')));
    expect(await screen.findByText('Updated Computer')).toBeInTheDocument();

    fireEvent.click(screen.getAllByRole('button', { name: 'Duplicate' })[0]);
    await waitFor(() => expect(mockFetchManualTargets).toHaveBeenCalled());
    expect(screen.getByLabelText('Copy Robot binding')).toBeChecked();
    fireEvent.mouseDown(screen.getByLabelText('Connection target'));
    fireEvent.click(await screen.findByText('Target A (office-a)'));
    fireEvent.click(last(screen.getAllByText('OK')));
    expect(await screen.findByText('prod Copy')).toBeInTheDocument();
    expect(mockInvoke).toHaveBeenCalledWith('duplicate_computer_instance', {
      request: {
        sourceId: 'computer-a',
        name: 'Updated Computer Copy',
        description: 'Updated description',
        copyRobotBinding: true,
        connectionTargetId: 'target-a',
        skillHomeMode: 'empty',
      },
    });

    const originalComputerCard = () => {
      const card = screen.getByText('computer-a').closest<HTMLElement>('.ant-card');
      expect(card).not.toBeNull();
      return card!;
    };

    await waitFor(() => {
      expect(within(originalComputerCard()).getByRole('button', { name: 'Stop' })).toBeEnabled();
    });
    fireEvent.click(within(originalComputerCard()).getByRole('button', { name: 'Stop' }));
    await waitFor(() => {
      expect(within(originalComputerCard()).getByText('Not Running')).toBeInTheDocument();
      expect(within(originalComputerCard()).getByRole('button', { name: 'Start' })).toBeEnabled();
    });
    fireEvent.click(within(originalComputerCard()).getByRole('button', { name: 'Start' }));
    await waitFor(() => {
      expect(within(originalComputerCard()).getByText('Running')).toBeInTheDocument();
      expect(within(originalComputerCard()).getByRole('button', { name: 'Delete' })).toBeEnabled();
    });
  }, 80000);

  it('deletes a Computer from the list actions after confirmation', async () => {
    mockInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'list_computer_instances') return mockComputerInstances;
      if (cmd === 'delete_computer_instance') return null;
      return null;
    });

    render(<Computer />);

    const computerId = await screen.findByText('computer-a');
    const computerCard = computerId.closest<HTMLElement>('.ant-card');
    expect(computerCard).not.toBeNull();
    await waitFor(() => {
      expect(within(computerCard!).getByRole('button', { name: 'Delete' })).toBeEnabled();
    });

    fireEvent.click(within(computerCard!).getByRole('button', { name: 'Delete' }));
    const deleteConfirmation = await screen.findByText('Delete this Computer?');
    const deletePopover = deleteConfirmation.closest<HTMLElement>('.ant-popover');
    expect(deletePopover).not.toBeNull();
    fireEvent.click(within(deletePopover!).getByRole('button', { name: 'OK' }));
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('delete_computer_instance', { id: 'computer-a' });
      expect(screen.queryByText('computer-a')).not.toBeInTheDocument();
    });
  });

  it('prompts for consecutive missing values and retries only the original start action', async () => {
    const stopped = {
      ...mockComputerInstances[0],
      running: false,
      connected: false,
      runtime: runtimeSnapshot({ lifecycle: 'shutdown' }),
    };
    let startAttempts = 0;
    mockInvoke.mockImplementation(async (cmd, args) => {
      if (cmd === 'list_computer_instances') return [stopped];
      if (cmd === 'start_computer_instance') {
        startAttempts += 1;
        if (startAttempts === 1) {
          throw {
            code: 'missing_secret',
            input_id: 'secret-a',
            env_hint: 'A2C_SMCP_secret_a',
            message: 'Required secret input is unresolved',
          };
        }
        if (startAttempts === 2) {
          throw {
            code: 'missing_input',
            input_id: 'value-b',
            env_hint: 'A2C_SMCP_value_b',
            message: 'Required input is unresolved',
          };
        }
        return {
          ...stopped,
          running: true,
          runtime: runtimeSnapshot({ snapshot_revision: 2 }),
        };
      }
      if (cmd === 'get_input') {
        const inputId = (args as { id?: string } | undefined)?.id;
        return inputId === 'secret-a' ? {
          type: 'PromptString',
          id: 'secret-a',
          label: 'Secret A',
          password: true,
        } : {
          type: 'PromptString',
          id: 'value-b',
          label: 'Value B',
          password: false,
        };
      }
      if (cmd === 'set_input_value') return null;
      if (cmd === 'list_input_values') return { 'api-key': { configured: true } };
      return null;
    });

    render(<Computer />);
    fireEvent.click((await screen.findAllByRole('button', { name: 'Start' }))[0]);

    expect(await screen.findByText('Secret required to start')).toBeInTheDocument();
    const input = await screen.findByPlaceholderText('Enter value');
    fireEvent.change(input, { target: { value: 'secret-a-value' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(startAttempts).toBe(2));
    expect(await screen.findByText('Input required to start')).toBeInTheDocument();
    const secondInput = await screen.findByPlaceholderText('Enter value');
    expect(secondInput).toHaveAttribute('type', 'text');
    expect(secondInput).toHaveValue('');
    expect(screen.queryByDisplayValue('secret-a-value')).not.toBeInTheDocument();
    fireEvent.change(secondInput, { target: { value: 'value-b-value' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(startAttempts).toBe(3));
    expect(await screen.findByText('Running')).toBeInTheDocument();
    expect(mockInvoke).toHaveBeenCalledWith('set_input_value', {
      instanceId: 'computer-a',
      id: 'secret-a',
      value: 'secret-a-value',
    });
    expect(mockInvoke).toHaveBeenCalledWith('set_input_value', {
      instanceId: 'computer-a',
      id: 'value-b',
      value: 'value-b-value',
    });
  }, 80000);
});
