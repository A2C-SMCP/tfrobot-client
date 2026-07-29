import { fireEvent, render, screen } from '../helpers/render';
import { ComputerRuntime } from '@/components/Computer/ComputerRuntime';
import type { ComputerInstance } from '@/stores/computerStore';
import { useRuntimeStore } from '@/stores/runtimeStore';
import { runtimeSnapshot } from '../helpers/store';

vi.mock('@/components/McpConfig/McpRuntimeControls', () => ({
  McpRuntimeControls: ({
    capability,
  }: {
    capability: { enabled: boolean };
  }) => (
    <div data-testid="runtime-mcp" data-disabled={String(!capability.enabled)} />
  ),
}));

function instance(overrides: Partial<ComputerInstance> = {}): ComputerInstance {
  return {
    id: 'computer-a',
    name: 'Computer A',
    status: 'running',
    connectionStatus: 'disconnected',
    connectionPolicy: { target: { type: 'manual_smcp', id: 'target-a' }, auto_connect: false },
    mcpServerCount: 3,
    runtime: runtimeSnapshot({
      lifecycle: 'started',
      generation: 2,
      config_revision: 4,
      capability_revision: 7,
      mcp_servers: 3,
      active_mcp_servers: 2,
      tools: 12,
      skills: 5,
    }),
    ...overrides,
  };
}

describe('ComputerRuntime', () => {
  beforeEach(() => {
    useRuntimeStore.getState().reset();
  });

  it('renders SDK runtime state and exposes lifecycle operations separately', () => {
    const onStartStop = vi.fn();
    const onRestart = vi.fn();
    const onConnect = vi.fn();

    render(
      <ComputerRuntime
        instance={instance()}
        loading={false}
        canConnect
        onStartStop={onStartStop}
        onRestart={onRestart}
        onConnect={onConnect}
        onDisconnect={vi.fn()}
        onViewLogs={vi.fn()}
      />,
    );

    expect(screen.getByText('Running')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Reload$/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByText('Advanced Runtime Diagnostics'));
    expect(screen.getByText('Started')).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: 'Runtime Incarnation 1' })).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: 'Runtime Generation 2' })).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: 'Config Revision 4' })).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: 'Capability Revision 7' })).toBeInTheDocument();
    expect(screen.getByTestId('runtime-mcp')).toHaveAttribute('data-disabled', 'false');

    fireEvent.click(screen.getByRole('button', { name: /Stop$/ }));
    fireEvent.click(screen.getByRole('button', { name: /Restart$/ }));
    fireEvent.click(screen.getByRole('button', { name: /Connect$/ }));

    expect(onStartStop).toHaveBeenCalledOnce();
    expect(onRestart).toHaveBeenCalledOnce();
    expect(onConnect).toHaveBeenCalledOnce();
  }, 30_000);

  it('shows snapshot revision and recent accepted runtime events', () => {
    const runtime = runtimeSnapshot({
      lifecycle: 'started',
      snapshot_revision: 9,
      config_revision: 4,
    });
    useRuntimeStore.setState({
      eventsByInstance: {
        'computer-a': [{
          instance_id: 'computer-a',
          cause: { kind: 'config_revision_bumped', revision: 4 },
          snapshot: runtime,
          connection: { present: false, revision: 0, context: null },
          received_at: '2026-07-17T10:00:00.000Z',
        }],
      },
    });

    render(
      <ComputerRuntime
        instance={instance({ runtime })}
        loading={false}
        canConnect
        onStartStop={vi.fn()}
        onRestart={vi.fn()}
        onConnect={vi.fn()}
        onDisconnect={vi.fn()}
        onViewLogs={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByText('Advanced Runtime Diagnostics'));
    expect(screen.getByRole('cell', { name: 'Snapshot Revision 9' })).toBeInTheDocument();
    expect(screen.getByText('Recent Runtime Events')).toBeInTheDocument();
    expect(screen.getByText('Config revision changed to 4')).toBeInTheDocument();
  });

  it('shows a safe structured Runtime error and keeps technical detail in diagnostics', () => {
    const onViewLogs = vi.fn();
    render(
      <ComputerRuntime
        instance={instance({
          status: 'error',
          runtime: runtimeSnapshot({
            lifecycle: 'error',
            last_error: 'runtime boot failed',
            problems: [{
              id: 'sdk:2:runtime_error',
              source: 'sdk',
              operation: 'runtime',
              severity: 'error',
              affected_capabilities: [{ kind: 'runtime' }],
              occurred_at: '2026-07-29T02:00:00Z',
              current: true,
              message: 'sdk_runtime_error',
              recommended_actions: ['start_runtime', 'view_logs'],
              technical_detail: 'runtime boot failed',
            }],
          }),
        })}
        loading={false}
        canConnect={false}
        connectDisabledReason="Start the Computer first"
        onStartStop={vi.fn()}
        onRestart={vi.fn()}
        onConnect={vi.fn()}
        onDisconnect={vi.fn()}
        onViewLogs={onViewLogs}
      />,
    );

    expect(screen.getByText('Core Runtime capabilities are unavailable.')).toBeInTheDocument();
    expect(screen.getByText('Affected: core Runtime')).toBeInTheDocument();
    expect(screen.queryByText('runtime boot failed')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'View logs' }));
    expect(onViewLogs).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByText('Advanced Runtime Diagnostics'));
    expect(screen.getByText('runtime boot failed')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Restart$/ })).toBeDisabled();
    expect(screen.getByRole('button', { name: /Connect$/ })).toBeDisabled();
    expect(screen.getByTestId('runtime-mcp')).toHaveAttribute('data-disabled', 'true');
  });

  it('renders degraded MCP impact separately and explains an unavailable recovery action', () => {
    render(
      <ComputerRuntime
        instance={instance({
          status: 'degraded',
          runtime: runtimeSnapshot({
            lifecycle: 'degraded',
            problems: [{
              id: 'mcp:2:server-a:start',
              source: 'mcp',
              operation: 'start',
              severity: 'degraded',
              affected_capabilities: [{
                kind: 'mcp_server',
                bundle_id: 'server-a',
                name: 'Browser MCP',
              }],
              occurred_at: '2026-07-29T02:00:00Z',
              current: true,
              message: 'mcp_start_failed',
              recommended_actions: ['restart_runtime', 'view_logs'],
              technical_detail: 'process exited',
            }],
            actions: {
              ...runtimeSnapshot({ lifecycle: 'degraded' }).actions,
              restart: { enabled: false, disabled_reason: 'transition_in_progress' },
            },
          }),
        })}
        loading={false}
        canConnect={false}
        onStartStop={vi.fn()}
        onRestart={vi.fn()}
        onConnect={vi.fn()}
        onDisconnect={vi.fn()}
        onViewLogs={vi.fn()}
      />,
    );

    expect(screen.getByText('The Runtime is available, but an MCP server could not start.'))
      .toBeInTheDocument();
    expect(screen.getByText('Affected: MCP server Browser MCP')).toBeInTheDocument();
    expect(screen.getByText(
      'Restart unavailable: Wait for the current Runtime operation to finish.',
    )).toBeInTheDocument();
  });

  it('disables every conflicting action while the SDK lifecycle is transitional', () => {
    render(
      <ComputerRuntime
        instance={instance({
          status: 'running',
          runtime: runtimeSnapshot({ lifecycle: 'connecting' }),
        })}
        loading={false}
        canConnect={false}
        onStartStop={vi.fn()}
        onRestart={vi.fn()}
        onConnect={vi.fn()}
        onDisconnect={vi.fn()}
        onViewLogs={vi.fn()}
      />,
    );

    expect(screen.getByRole('button', { name: /Stop$/ })).toBeDisabled();
    expect(screen.getByRole('button', { name: /Restart$/ })).toBeDisabled();
    expect(screen.getByRole('button', { name: /Connect$/ })).toBeDisabled();
    expect(screen.getAllByText('Wait for the current Runtime operation to finish.')).toHaveLength(1);
    expect(screen.getByTestId('runtime-mcp')).toHaveAttribute('data-disabled', 'true');
  });

  it.each([
    ['connected', 'running'],
    ['degraded', 'degraded'],
  ] as const)(
    'offers disconnect while the client connection authority is present in %s lifecycle',
    (lifecycle, status) => {
      const onDisconnect = vi.fn();
      render(
        <ComputerRuntime
          instance={instance({
            status,
            connectionStatus: 'disconnected',
            clientConnectionPresent: true,
            runtime: runtimeSnapshot({ lifecycle }),
          })}
          loading={false}
          canConnect={false}
          onStartStop={vi.fn()}
          onRestart={vi.fn()}
          onConnect={vi.fn()}
          onDisconnect={onDisconnect}
          onViewLogs={vi.fn()}
        />,
      );

      const disconnect = screen.getByRole('button', { name: /Disconnect$/ });
      expect(disconnect).toBeEnabled();
      expect(screen.queryByRole('button', { name: /Connect$/ })).not.toBeInTheDocument();
      fireEvent.click(disconnect);
      expect(onDisconnect).toHaveBeenCalledOnce();
    },
  );

  it.each([
    ['connecting', 'connect', 'Connect'],
    ['disconnecting', 'disconnect', 'Disconnect'],
  ] as const)(
    'renders backend-owned %s state and disables conflicting connection actions',
    (connectionStatus, operation, buttonName) => {
      render(
        <ComputerRuntime
          instance={instance({
            connectionStatus,
            connectionState: {
              status: connectionStatus,
              present: connectionStatus === 'disconnecting',
              revision: 4,
            context: null,
            operation,
            operation_target: null,
            last_error: null,
              actions: {
                connect: { enabled: false, disabled_reason: 'transition_in_progress' },
                disconnect: { enabled: false, disabled_reason: 'transition_in_progress' },
              },
            },
          })}
          loading={false}
          canConnect
          onStartStop={vi.fn()}
          onRestart={vi.fn()}
          onConnect={vi.fn()}
          onDisconnect={vi.fn()}
          onViewLogs={vi.fn()}
        />,
      );

      expect(screen.getByText(connectionStatus === 'connecting' ? 'Connecting' : 'Disconnecting'))
        .toBeInTheDocument();
      const action = screen.getByRole('button', { name: new RegExp(`${buttonName}$`) });
      expect(action).toBeDisabled();
      expect(action).toHaveClass('ant-btn-loading');
    },
  );

  it('offers disconnect cleanup for a disconnected orphan transport capability', () => {
    const onDisconnect = vi.fn();
    render(
      <ComputerRuntime
        instance={instance({
          connectionState: {
            status: 'disconnected',
            present: false,
            revision: 8,
          context: null,
          operation: null,
          operation_target: null,
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
        })}
        loading={false}
        canConnect={false}
        onStartStop={vi.fn()}
        onRestart={vi.fn()}
        onConnect={vi.fn()}
        onDisconnect={onDisconnect}
        onViewLogs={vi.fn()}
      />,
    );

    expect(screen.queryByRole('button', { name: /Connect$/ })).not.toBeInTheDocument();
    const disconnect = screen.getByRole('button', { name: /Disconnect$/ });
    expect(disconnect).toBeEnabled();
    fireEvent.click(disconnect);
    expect(onDisconnect).toHaveBeenCalledOnce();
  });
});
