import { invoke } from '@tauri-apps/api/core';
import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { RobotConnectionPanel } from '@/components/RobotConnectionPanel';
import {
  useComputerStore,
  type ComputerConnectionTarget,
  type RobotBindingMetadata,
} from '@/stores/computerStore';
import { managerContextScope, useManagerStore, type ManagerContextSnapshot } from '@/stores/managerStore';
import { runtimeSnapshot } from '../helpers/store';

const mockedInvoke = vi.mocked(invoke);
const managerContextKey = {
  environment: 'staging' as const,
  accountId: 'org-1:account-23',
  organizationId: 'org-1',
};

const managerTarget = (
  employeeId: number,
  lastResolvedRobotAccountId?: string,
  contextKey = managerContextKey,
) => ({
  type: 'manager_robot' as const,
  contextKey,
  employeeId,
  lastResolvedRobotAccountId,
});

function setComputer(
  target: ComputerConnectionTarget | null = null,
  robotBinding: RobotBindingMetadata | null = null,
) {
  useComputerStore.setState({
    instances: [
      {
        id: 'computer-a',
        name: 'Computer A',
        status: 'running',
        connectionStatus: 'disconnected',
        connectionPolicy: { target, auto_connect: false },
        robotBinding,
        mcpServerCount: 0,
        runtime: runtimeSnapshot(),
      },
    ],
    loading: false,
    error: null,
    selectedInstanceId: 'computer-a',
  });
}

function setManagerRobots(contextKey = managerContextKey) {
  const context: ManagerContextSnapshot = {
    revision: 1,
    authState: 'authenticated',
    environment: 'staging',
    contextKey,
    user: { id: '1', nickname: 'User', email: '', phone: '' },
    account: {
      id: contextKey.accountId,
      name: 'acct',
      nickname: 'User',
      avatar: '',
      employeeNo: '',
    },
    organization: { id: contextKey.organizationId, name: 'Org', organizationType: 'team' },
    permissions: [],
  };
  useManagerStore.getState().applyContext(context);
  const scope = managerContextScope(context)!;
  useManagerStore.setState((state) => ({
    employeeResources: {
      ...state.employeeResources,
      [scope]: {
        ...state.employeeResources[scope],
        employees: [
      {
        id: 24,
        name: 'Deleted Robot',
        status: 'deleted',
        robotAccountId: '2424',
      },
      {
        id: 25,
        name: 'Running Robot',
        status: 'running',
        robotAccountId: '2525',
        robotId: 'robot-25',
        templateDisplayName: 'UAT',
        templateType: 'tfrserver',
        namespace: 'org-a',
      },
        ],
        lastFetchAt: Date.now(),
      },
    },
  }));
}

function managerPolicyResponse() {
  return {
    id: 'computer-a',
    name: 'Computer A',
    running: true,
    connected: false,
    mcp_server_count: 0,
    runtime: runtimeSnapshot(),
    robot_binding: null,
    connection_policy: {
      target: managerTarget(25, '2525'),
      auto_connect: false,
    },
    connection: null,
  };
}

describe('RobotConnectionPanel', () => {
  beforeEach(() => {
    useManagerStore.setState(useManagerStore.getInitialState(), true);
    setComputer();
    mockedInvoke.mockReset();
  });

  it('shows a Manager login guide and never loads Manual SMCP targets', async () => {
    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    expect(await screen.findByText(/No Manager account is signed in/i)).toBeInTheDocument();
    expect(screen.queryByText(/Manual SMCP/i)).not.toBeInTheDocument();
    expect(mockedInvoke).not.toHaveBeenCalledWith('list_manual_smcp_targets');
    expect(mockedInvoke).not.toHaveBeenCalledWith('get_connection_status', expect.anything());
  });

  it('hides a persisted legacy Manual target without clearing it', () => {
    setComputer({ type: 'manual_smcp', id: 'legacy-target' });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    expect(screen.queryByText('legacy-target')).not.toBeInTheDocument();
    expect(screen.getByRole('switch', { name: 'Auto Connect' })).toBeDisabled();
    expect(mockedInvoke).not.toHaveBeenCalledWith('update_computer_connection_policy', expect.anything());
  });

  it('lists Manager Robots as read-only resources when signed in', async () => {
    setManagerRobots();

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    expect(await screen.findByText('Running Robot')).toBeInTheDocument();
    expect(screen.getByText('robot-25')).toBeInTheDocument();
    expect(mockedInvoke).not.toHaveBeenCalledWith('connect_computer_connection_target', expect.anything());
  });

  it('only exposes a running Manager Robot as a selectable policy target', async () => {
    setManagerRobots();
    mockedInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'update_computer_connection_policy') return managerPolicyResponse();
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    fireEvent.mouseDown(await screen.findByRole('combobox'));
    expect(screen.queryByText('Deleted Robot (deleted)')).not.toBeInTheDocument();
    fireEvent.click(await screen.findByText('Running Robot (running / UAT / tfrserver / org-a)'));

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('update_computer_connection_policy', {
        request: {
          id: 'computer-a',
          target: managerTarget(25, '2525'),
          autoConnect: false,
        },
      });
    });
  });

  it('rolls back a Manager Robot selection when policy persistence fails', async () => {
    setManagerRobots();
    mockedInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'update_computer_connection_policy') throw new Error('save failed');
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    fireEvent.mouseDown(await screen.findByRole('combobox'));
    fireEvent.click(await screen.findByText('Running Robot (running / UAT / tfrserver / org-a)'));

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('update_computer_connection_policy', expect.anything());
      expect(screen.getByText('Select a Manager Robot')).toBeInTheDocument();
    });
    expect(screen.getByRole('switch', { name: 'Auto Connect' })).toBeDisabled();
  });

  it('does not persist diagnostic metadata merely because the resource list loaded', async () => {
    setComputer(managerTarget(25));
    setManagerRobots();
    mockedInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'update_computer_connection_policy') return managerPolicyResponse();
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    await screen.findByText('Running Robot');
    expect(mockedInvoke).not.toHaveBeenCalledWith(
      'update_computer_connection_policy',
      expect.anything(),
    );
  });

  it('does not present a target from another Context as selected or connectable', async () => {
    setComputer(managerTarget(25, 'old-diagnostic'));
    setManagerRobots({
      environment: 'staging',
      accountId: 'org-2:account-99',
      organizationId: 'org-2',
    });

    render(<RobotConnectionPanel instanceId="computer-a" onNavigate={vi.fn()} />);

    expect(await screen.findByText('Running Robot')).toBeInTheDocument();
    expect(screen.getByText('Select a Manager Robot')).toBeInTheDocument();
    expect(screen.getByText('This Robot binding is dormant')).toBeInTheDocument();
    expect(screen.getByRole('switch', { name: 'Auto Connect' })).toBeDisabled();
    expect(mockedInvoke).not.toHaveBeenCalledWith(
      'update_computer_connection_policy',
      expect.anything(),
    );
  });

  it('lets a migrated needs-rebind profile explicitly select a current Context Robot', async () => {
    setComputer(null, {
      state: 'needs_rebind',
      employee_id: 7,
      last_resolved_robot_account_id: 'legacy-account',
      robot_name: 'Legacy Robot',
    });
    setManagerRobots();
    mockedInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'update_computer_connection_policy') return managerPolicyResponse();
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" />);

    expect(await screen.findByText('This migrated binding needs a Robot selection'))
      .toBeInTheDocument();
    fireEvent.mouseDown(screen.getByRole('combobox'));
    fireEvent.click(await screen.findByText('Running Robot (running / UAT / tfrserver / org-a)'));

    await waitFor(() => expect(mockedInvoke).toHaveBeenCalledWith(
      'update_computer_connection_policy',
      {
        request: {
          id: 'computer-a',
          target: managerTarget(25, '2525'),
          autoConnect: false,
        },
      },
    ));
  });

  it('requires an explicit action to reactivate a dormant same-Context binding', async () => {
    setComputer(managerTarget(25, '2525'), {
      context_key: managerContextKey,
      state: 'dormant',
      employee_id: 25,
      last_resolved_robot_account_id: '2525',
      robot_name: 'Running Robot',
    });
    setManagerRobots();
    mockedInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'update_computer_connection_policy') return managerPolicyResponse();
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" />);

    expect(await screen.findByText('This Robot binding is dormant')).toBeInTheDocument();
    expect(screen.getByRole('switch', { name: 'Auto Connect' })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Reactivate' }));

    await waitFor(() => expect(mockedInvoke).toHaveBeenCalledWith(
      'update_computer_connection_policy',
      {
        request: {
          id: 'computer-a',
          target: managerTarget(25, '2525'),
          autoConnect: false,
        },
      },
    ));
  });

  it('marks a no-longer-visible active target as permission-revoked and allows reselection', async () => {
    setComputer(managerTarget(99, 'stale-account'), {
      context_key: managerContextKey,
      state: 'active',
      employee_id: 99,
      last_resolved_robot_account_id: 'stale-account',
      robot_name: 'Revoked Robot',
    });
    setManagerRobots();
    mockedInvoke.mockImplementation(async (cmd) => {
      if (cmd === 'update_computer_connection_policy') return managerPolicyResponse();
      return null;
    });

    render(<RobotConnectionPanel instanceId="computer-a" />);

    expect(await screen.findByText('The saved Robot is no longer visible')).toBeInTheDocument();
    expect(screen.getByRole('switch', { name: 'Auto Connect' })).toBeDisabled();
    fireEvent.mouseDown(screen.getByRole('combobox'));
    fireEvent.click(await screen.findByText('Running Robot (running / UAT / tfrserver / org-a)'));

    await waitFor(() => expect(mockedInvoke).toHaveBeenCalledWith(
      'update_computer_connection_policy',
      expect.objectContaining({
        request: expect.objectContaining({ target: managerTarget(25, '2525') }),
      }),
    ));
  });
});
