import { fireEvent, render, screen } from '../helpers/render';
import { ComputerRuntime } from '@/components/Computer/ComputerRuntime';
import type { ComputerInstance } from '@/stores/computerStore';
import { useRuntimeStore } from '@/stores/runtimeStore';
import { runtimeSnapshot } from '../helpers/store';

vi.mock('@/components/McpConfig/McpRuntimeControls', () => ({
  McpRuntimeControls: ({ disabled }: { disabled: boolean }) => (
    <div data-testid="runtime-mcp" data-disabled={String(disabled)} />
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
    const onReload = vi.fn();
    const onConnect = vi.fn();

    render(
      <ComputerRuntime
        instance={instance()}
        loading={false}
        canConnect
        onStartStop={onStartStop}
        onRestart={onRestart}
        onReload={onReload}
        onConnect={onConnect}
        onDisconnect={vi.fn()}
      />,
    );

    expect(screen.getByText('Started')).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: 'Runtime Generation 2' })).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: 'Config Revision 4' })).toBeInTheDocument();
    expect(screen.getByRole('cell', { name: 'Capability Revision 7' })).toBeInTheDocument();
    expect(screen.getByTestId('runtime-mcp')).toHaveAttribute('data-disabled', 'false');

    fireEvent.click(screen.getByRole('button', { name: /Stop$/ }));
    fireEvent.click(screen.getByRole('button', { name: /Restart$/ }));
    fireEvent.click(screen.getByRole('button', { name: /Reload$/ }));
    fireEvent.click(screen.getByRole('button', { name: /Connect$/ }));

    expect(onStartStop).toHaveBeenCalledOnce();
    expect(onRestart).toHaveBeenCalledOnce();
    expect(onReload).toHaveBeenCalledOnce();
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
        onReload={vi.fn()}
        onConnect={vi.fn()}
        onDisconnect={vi.fn()}
      />,
    );

    expect(screen.getByRole('cell', { name: 'Snapshot Revision 9' })).toBeInTheDocument();
    expect(screen.getByText('Recent Runtime Events')).toBeInTheDocument();
    expect(screen.getByText('Config revision changed to 4')).toBeInTheDocument();
  });

  it('surfaces structured runtime failures and disables runtime-only MCP actions', () => {
    render(
      <ComputerRuntime
        instance={instance({
          status: 'error',
          runtime: runtimeSnapshot({
            lifecycle: 'error',
            last_error: 'runtime boot failed',
          }),
        })}
        loading={false}
        canConnect={false}
        connectDisabledReason="Start the Computer first"
        onStartStop={vi.fn()}
        onRestart={vi.fn()}
        onReload={vi.fn()}
        onConnect={vi.fn()}
        onDisconnect={vi.fn()}
      />,
    );

    expect(screen.getByText('runtime boot failed')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Restart$/ })).toBeDisabled();
    expect(screen.getByRole('button', { name: /Connect$/ })).toBeDisabled();
    expect(screen.getByTestId('runtime-mcp')).toHaveAttribute('data-disabled', 'true');
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
        onReload={vi.fn()}
        onConnect={vi.fn()}
        onDisconnect={vi.fn()}
      />,
    );

    expect(screen.getByRole('button', { name: /Stop$/ })).toBeDisabled();
    expect(screen.getByRole('button', { name: /Restart$/ })).toBeDisabled();
    expect(screen.getByRole('button', { name: /Reload$/ })).toBeDisabled();
    expect(screen.getByRole('button', { name: /Connect$/ })).toBeDisabled();
    expect(screen.getByTestId('runtime-mcp')).toHaveAttribute('data-disabled', 'true');
  });
});
