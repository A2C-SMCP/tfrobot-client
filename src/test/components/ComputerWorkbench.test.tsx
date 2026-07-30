import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { ComputerWorkbench } from '@/components/Computer/ComputerWorkbench';
import type { ResolvedComputerConnection } from '@/components/Computer/computerActions';
import type { ComputerInstance } from '@/stores/computerStore';
import { runtimeSnapshot } from '../helpers/store';

vi.mock('@/components/Computer/ComputerRuntime', () => ({
  ComputerRuntime: ({
    instance,
    connection,
  }: {
    instance: ComputerInstance;
    connection: ResolvedComputerConnection;
  }) => (
    <div
      data-testid="runtime-section"
      data-operation={connection.operation ?? ''}
      data-operation-target={connection.operationTarget?.target_id ?? ''}
    >
      {instance.id}
    </div>
  ),
}));
vi.mock('@/components/Computer/SkillsTab', () => ({
  SkillsTab: ({ instanceId }: { instanceId: string }) => (
    <div data-testid="skills-section">{instanceId}</div>
  ),
}));
vi.mock('@/components/DesktopResources', () => ({
  DesktopResources: ({ instanceId }: { instanceId: string }) => (
    <div data-testid="resources-section">{instanceId}</div>
  ),
}));
vi.mock('@/components/DebugPanel', () => ({
  DebugPanel: ({ instanceId }: { instanceId: string }) => (
    <div data-testid="debug-section">{instanceId}</div>
  ),
}));
vi.mock('@/components/LogViewer', () => ({
  LogViewer: ({ instanceId }: { instanceId: string }) => (
    <div data-testid="logs-section">{instanceId}</div>
  ),
}));
vi.mock('@/components/Computer/RuntimeDiagnostics', () => ({
  RuntimeDiagnostics: () => <div data-testid="advanced-diagnostics" />,
}));

function instance(overrides: Partial<ComputerInstance> = {}): ComputerInstance {
  return {
    id: 'computer-a',
    name: 'Computer A',
    description: 'Primary runtime',
    status: 'running',
    connectionStatus: 'disconnected',
    connectionPolicy: {
      target: { type: 'manual_smcp', id: 'target-a' },
      auto_connect: false,
    },
    mcpServerCount: 1,
    runtime: runtimeSnapshot({ lifecycle: 'started' }),
    ...overrides,
  };
}

function renderWorkbench(
  computer = instance(),
  initialSection: React.ComponentProps<typeof ComputerWorkbench>['initialSection'] = 'top',
) {
  const props: React.ComponentProps<typeof ComputerWorkbench> = {
    instance: computer,
    loading: false,
    initialSection,
    onBack: vi.fn(),
    onOpenSettings: vi.fn(),
    onEdit: vi.fn(),
    onDelete: vi.fn().mockResolvedValue(undefined),
    onStartStop: vi.fn(),
    onRestart: vi.fn(),
    onConnect: vi.fn(),
    onDisconnect: vi.fn(),
  };
  render(<ComputerWorkbench {...props} />);
  return props;
}

describe('ComputerWorkbench', () => {
  it('renders an accessible icon-only back action and one longitudinal workbench', () => {
    const props = renderWorkbench();

    const back = screen.getByRole('button', { name: 'Back to Computers' });
    expect(back).toHaveTextContent('');
    fireEvent.click(back);
    expect(props.onBack).toHaveBeenCalledOnce();

    expect(screen.getByLabelText('Computer runtime workbench')).toBeInTheDocument();
    expect(screen.getByTestId('runtime-section')).toHaveTextContent('computer-a');
    expect(screen.getByTestId('skills-section')).toHaveTextContent('computer-a');
    expect(screen.getByTestId('resources-section')).toHaveTextContent('computer-a');
    expect(screen.queryByRole('tab', { name: 'Runtime' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Open Computer settings' })).toBeInTheDocument();
  });

  it('uses Runtime actions as the authority for the primary operation', () => {
    const props = renderWorkbench();

    fireEvent.click(screen.getByRole('button', { name: 'Stop' }));
    expect(props.onStartStop).toHaveBeenCalledOnce();
    expect(screen.getByRole('button', { name: 'Connect' })).toBeEnabled();
  });

  it('keeps a backend connect operation and target authoritative when policy changes', () => {
    renderWorkbench(instance({
      connectionStatus: 'connecting',
      connectionPolicy: {
        target: { type: 'manual_smcp', id: 'new-policy-target' },
        auto_connect: false,
      },
      connectionState: {
        status: 'connecting',
        present: false,
        revision: 2,
        context: null,
        operation: 'connect',
        operation_target: {
          source_type: 'manual_smcp',
          target_id: 'in-flight-target',
          employee_id: null,
        },
        last_error: null,
        actions: {
          connect: { enabled: false, disabled_reason: 'transition_in_progress' },
          disconnect: { enabled: false, disabled_reason: 'transition_in_progress' },
        },
      },
    }));

    expect(screen.getByText('Connecting')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Connect' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Connect' })).toHaveClass('ant-btn-loading');
    expect(screen.getByTestId('runtime-section')).toHaveAttribute(
      'data-operation-target',
      'in-flight-target',
    );
    expect(screen.getByText('Wait for the current connection operation to finish.'))
      .toBeInTheDocument();
  });

  it('shows disconnect during the backend-owned disconnection transition', () => {
    renderWorkbench(instance({
      connectionStatus: 'disconnecting',
      connectionState: {
        status: 'disconnecting',
        present: true,
        revision: 3,
        context: null,
        operation: 'disconnect',
        operation_target: {
          source_type: 'manual_smcp',
          target_id: 'target-a',
          employee_id: null,
        },
        last_error: null,
        actions: {
          connect: { enabled: false, disabled_reason: 'transition_in_progress' },
          disconnect: { enabled: false, disabled_reason: 'transition_in_progress' },
        },
      },
    }));

    expect(screen.getByText('Disconnecting')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Disconnect' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Disconnect' })).toHaveClass('ant-btn-loading');
  });

  it('projects a backend reconnect as an authoritative connecting operation', () => {
    renderWorkbench(instance({
      connectionStatus: 'connecting',
      connectionState: {
        status: 'connecting',
        present: true,
        revision: 4,
        context: null,
        operation: 'reconnect',
        operation_target: {
          source_type: 'manager_robot',
          target_id: 'manager:42',
          employee_id: 42,
        },
        last_error: null,
        actions: {
          connect: { enabled: false, disabled_reason: 'transition_in_progress' },
          disconnect: { enabled: false, disabled_reason: 'transition_in_progress' },
        },
      },
    }));

    expect(screen.getByRole('button', { name: 'Connect' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Connect' })).toHaveClass('ant-btn-loading');
    expect(screen.getByTestId('runtime-section')).toHaveAttribute(
      'data-operation',
      'reconnect',
    );
  });

  it('keeps backend orphan cleanup available while disconnected', () => {
    const props = renderWorkbench(instance({
      connectionStatus: 'disconnected',
      connectionState: {
        status: 'disconnected',
        present: false,
        revision: 5,
        context: null,
        operation: null,
        operation_target: null,
        last_error: null,
        actions: {
          connect: { enabled: true, disabled_reason: null },
          disconnect: { enabled: true, disabled_reason: null },
        },
      },
    }));

    const disconnect = screen.getByRole('button', { name: 'Disconnect' });
    expect(disconnect).toBeEnabled();
    fireEvent.click(disconnect);
    expect(props.onDisconnect).toHaveBeenCalledOnce();
  });

  it('maps a legacy logs destination to the expanded diagnostics region', async () => {
    renderWorkbench(instance(), 'logs');

    expect(await screen.findByTestId('logs-section')).toHaveTextContent('computer-a');
  });

  it('keeps delete behind the more menu and a second confirmation', async () => {
    const props = renderWorkbench();

    fireEvent.click(screen.getByRole('button', { name: 'More Computer actions' }));
    fireEvent.click(await screen.findByText('Delete'));
    expect(screen.getByText('Delete this Computer?')).toBeInTheDocument();
    expect(props.onDelete).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(props.onDelete).toHaveBeenCalledOnce());
  });
});
