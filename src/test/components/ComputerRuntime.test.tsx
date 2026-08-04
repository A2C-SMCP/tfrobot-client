import { fireEvent, render, screen } from '../helpers/render';
import { ComputerRuntime } from '@/components/Computer/ComputerRuntime';
import { resolveComputerConnection } from '@/components/Computer/computerActions';
import type { ComputerInstance } from '@/stores/computerStore';
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
    defaultSkillHome: '/app/computer_instances/computer-a/skill_home',
    configuredSkillHome: '/app/computer_instances/computer-a/skill_home',
    effectiveSkillHome: '/app/computer_instances/computer-a/skill_home',
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

function renderRuntime(
  computer = instance(),
  overrides: Partial<React.ComponentProps<typeof ComputerRuntime>> = {},
) {
  const props: React.ComponentProps<typeof ComputerRuntime> = {
    instance: computer,
    connection: resolveComputerConnection(computer),
    loading: false,
    onStartStop: vi.fn(),
    onRestart: vi.fn(),
    onConnect: vi.fn(),
    onDisconnect: vi.fn(),
    onViewLogs: vi.fn(),
    ...overrides,
  };
  render(<ComputerRuntime {...props} />);
  return props;
}

describe('ComputerRuntime', () => {
  it('allows Manager connect only for an active binding in the current Context', () => {
    const contextKey = {
      environment: 'staging' as const,
      accountId: '23',
      organizationId: '1',
    };
    const computer = instance({
      connectionPolicy: {
        target: {
          type: 'manager_robot',
          contextKey,
          employeeId: 1001,
        },
        auto_connect: false,
      },
      robotBinding: {
        context_key: contextKey,
        state: 'active',
        employee_id: 1001,
      },
    });

    expect(resolveComputerConnection(computer, contextKey).targetConnectable).toBe(true);
    expect(resolveComputerConnection(computer, {
      ...contextKey,
      accountId: '99',
    }).targetConnectable).toBe(false);
    expect(resolveComputerConnection({
      ...computer,
      robotBinding: { ...computer.robotBinding!, state: 'dormant' },
    }, contextKey).targetConnectable).toBe(false);
  });

  it('composes MCP runtime and active capability summary without duplicating header actions', () => {
    renderRuntime();

    expect(screen.queryByText('Connection')).not.toBeInTheDocument();
    const capabilitySummary = screen.getByText('Active capability summary');
    const runtimeMcp = screen.getByTestId('runtime-mcp');
    expect(
      capabilitySummary.compareDocumentPosition(runtimeMcp)
      & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(runtimeMcp).toHaveAttribute('data-disabled', 'false');
    expect(screen.getByText('12')).toBeInTheDocument();
    expect(screen.getByText('5')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Stop$/ })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Restart$/ })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Connect$/ })).not.toBeInTheDocument();
    expect(screen.queryByText('Advanced Runtime Diagnostics')).not.toBeInTheDocument();
  });

  it('shows a safe structured Runtime error and delegates log navigation', () => {
    const onViewLogs = vi.fn();
    renderRuntime(
      instance({
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
      }),
      {
        onViewLogs,
      },
    );

    expect(screen.getByText('Core Runtime capabilities are unavailable.')).toBeInTheDocument();
    expect(screen.getByText('Affected: core Runtime')).toBeInTheDocument();
    expect(screen.queryByText('runtime boot failed')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'View logs' }));
    expect(onViewLogs).toHaveBeenCalledOnce();
    expect(screen.getByTestId('runtime-mcp')).toHaveAttribute('data-disabled', 'true');
  });

  it('renders degraded MCP impact and explains an unavailable recovery action', () => {
    renderRuntime(instance({
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
    }));

    expect(screen.getByText('The Runtime is available, but an MCP server could not start.'))
      .toBeInTheDocument();
    expect(screen.getByText('Affected: MCP server Browser MCP')).toBeInTheDocument();
    expect(screen.getByText(
      'Restart unavailable: Wait for the current Runtime operation to finish.',
    )).toBeInTheDocument();
  });

  it('disables MCP lifecycle controls while the Runtime lifecycle is transitional', () => {
    renderRuntime(instance({
      runtime: runtimeSnapshot({ lifecycle: 'connecting' }),
    }));

    expect(screen.getByTestId('runtime-mcp')).toHaveAttribute('data-disabled', 'true');
  });
});
