import { invoke } from '@tauri-apps/api/core';
import { useComputerStore } from '@/stores/computerStore';
import { useDashboardStore } from '@/stores/dashboardStore';
import { useRuntimeStore } from '@/stores/runtimeStore';
import { runtimeSnapshot } from '../helpers/store';

const mockedInvoke = vi.mocked(invoke);

const baseStatus = {
  id: 'computer-a',
  name: 'Computer A',
  description: 'Primary',
  running: false,
  runtime: runtimeSnapshot({ lifecycle: 'shutdown' }),
  connected: false,
  mcp_server_count: 1,
  robot_binding: null,
  connection_policy: { target: null, auto_connect: false },
  connection: null,
};

const baseInstance = {
  status: 'stopped' as const,
  connectionStatus: 'disconnected' as const,
  connectionPolicy: { target: null, auto_connect: false },
  mcpServerCount: 0,
  runtime: runtimeSnapshot({ lifecycle: 'shutdown' }),
};

function resetStore() {
  useComputerStore.setState({
    instances: [],
    loading: false,
    error: null,
    selectedInstanceId: null,
    listRequestId: 0,
    mutationEpoch: 0,
    profileMutationVersions: {},
  });
}

describe('computerStore', () => {
  beforeEach(() => {
    useRuntimeStore.getState().reset();
    resetStore();
    useDashboardStore.getState().reset();
    mockedInvoke.mockReset();
  });

  it('creates a Computer and selects it', async () => {
    mockedInvoke.mockResolvedValueOnce(baseStatus);

    await useComputerStore.getState().createInstance({ name: ' Computer A ', description: ' Primary ' });

    expect(mockedInvoke).toHaveBeenCalledWith('create_computer_instance', {
      request: { name: 'Computer A', description: 'Primary' },
    });
    expect(useComputerStore.getState().instances[0]).toMatchObject({
      id: 'computer-a',
      name: 'Computer A',
      description: 'Primary',
    });
    expect(useComputerStore.getState().selectedInstanceId).toBe('computer-a');
  });

  it('updates a Computer name and description', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'Old', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    mockedInvoke.mockResolvedValueOnce({ ...baseStatus, name: 'New', description: null });

    await useComputerStore.getState().updateInstance('computer-a', { name: 'New', description: '   ' });

    expect(mockedInvoke).toHaveBeenCalledWith('rename_computer_instance', {
      request: { id: 'computer-a', name: 'New', description: undefined },
    });
    expect(useComputerStore.getState().instances[0].name).toBe('New');
    expect(useComputerStore.getState().instances[0].description).toBeUndefined();
  });

  it('does not let an older list response undo a profile update', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'Old', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    let resolveList!: (value: unknown) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveList = resolve;
      }))
      .mockResolvedValueOnce({
        ...baseStatus,
        name: 'New',
        runtime: runtimeSnapshot({ snapshot_revision: 2 }),
      });

    const fetch = useComputerStore.getState().fetchInstances();
    await useComputerStore.getState().updateInstance('computer-a', { name: 'New' });
    resolveList([{ ...baseStatus, name: 'Old' }]);
    await fetch;

    expect(useComputerStore.getState().instances[0].name).toBe('New');
  });

  it('preserves raw connection authority when status refetches during reconnect', async () => {
    const connectionContext = {
      profile_name: 'prod',
      url: 'https://smcp.example.com',
      office_id: 'office-a',
      computer_name: 'Computer A',
      connected_at: '2026-07-17T00:00:00Z',
      source_type: 'manager_robot',
    };
    mockedInvoke
      .mockResolvedValueOnce([{
        ...baseStatus,
        connected: true,
        client_connection_present: true,
        connection_context: connectionContext,
        connection: connectionContext,
        runtime: runtimeSnapshot({ lifecycle: 'joined_office' }),
      }])
      .mockResolvedValueOnce([{
        ...baseStatus,
        connected: false,
        client_connection_present: true,
        connection_context: connectionContext,
        connection: null,
        runtime: runtimeSnapshot({ lifecycle: 'connected', snapshot_revision: 2 }),
      }]);

    await useComputerStore.getState().fetchInstances();
    await useComputerStore.getState().fetchInstances();
    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({ lifecycle: 'joined_office', snapshot_revision: 3 }),
    );

    expect(useComputerStore.getState().instances[0]).toMatchObject({
      connectionStatus: 'connected',
      connectionProfile: 'prod',
      connectionUrl: 'https://smcp.example.com',
    });
  });

  it('keeps the newest profile mutation when responses complete in reverse order', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'Initial', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    let resolveFirst!: (value: unknown) => void;
    let resolveSecond!: (value: unknown) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveFirst = resolve;
      }))
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveSecond = resolve;
      }));

    const first = useComputerStore.getState().updateInstance('computer-a', { name: 'First' });
    const second = useComputerStore.getState().updateInstance('computer-a', { name: 'Second' });
    resolveSecond({
      ...baseStatus,
      name: 'Second',
      runtime: runtimeSnapshot({ snapshot_revision: 2 }),
    });
    await second;
    resolveFirst({ ...baseStatus, name: 'First' });
    await first;

    expect(useComputerStore.getState().instances[0].name).toBe('Second');
  });

  it('merges a delayed runtime action without reverting newer profile fields', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'Initial', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    let resolveStart!: (value: unknown) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveStart = resolve;
      }))
      .mockResolvedValueOnce({
        ...baseStatus,
        name: 'Renamed',
        runtime: runtimeSnapshot({ snapshot_revision: 2 }),
      });

    const start = useComputerStore.getState().startInstance('computer-a');
    await useComputerStore.getState().updateInstance('computer-a', { name: 'Renamed' });
    resolveStart({
      ...baseStatus,
      name: 'Initial',
      running: true,
      runtime: runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 3 }),
    });
    await start;

    expect(useComputerStore.getState().instances[0]).toMatchObject({
      name: 'Renamed',
      status: 'running',
      runtime: { snapshot_revision: 3 },
    });
  });

  it('duplicates a Computer with Robot binding and connection target options', async () => {
    mockedInvoke.mockResolvedValueOnce({ ...baseStatus, id: 'computer-copy', name: 'Computer A Copy' });

    await useComputerStore.getState().duplicateInstance({
      sourceId: 'computer-a',
      name: 'Computer A Copy',
      description: 'Copy',
      copyRobotBinding: true,
      connectionTargetId: 'target-a',
      skillHomeMode: 'copy',
    });

    expect(mockedInvoke).toHaveBeenCalledWith('duplicate_computer_instance', {
      request: {
        sourceId: 'computer-a',
        name: 'Computer A Copy',
        description: 'Copy',
        copyRobotBinding: true,
        connectionTargetId: 'target-a',
        skillHomeMode: 'copy',
      },
    });
    expect(useComputerStore.getState().selectedInstanceId).toBe('computer-copy');
  });

  it('deletes a selected Computer and falls back to the first remaining instance', async () => {
    useComputerStore.setState({
      instances: [
        { id: 'computer-a', name: 'A', ...baseInstance },
        { id: 'computer-b', name: 'B', ...baseInstance },
      ],
      selectedInstanceId: 'computer-a',
    });
    mockedInvoke.mockResolvedValueOnce(null);

    await useComputerStore.getState().deleteInstance('computer-a');

    expect(mockedInvoke).toHaveBeenCalledWith('delete_computer_instance', { id: 'computer-a' });
    expect(useComputerStore.getState().instances.map((instance) => instance.id)).toEqual(['computer-b']);
    expect(useComputerStore.getState().selectedInstanceId).toBe('computer-b');
  });

  it('does not resurrect a deleted Computer from an older list request', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    let resolveList!: (value: unknown) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveList = resolve;
      }))
      .mockResolvedValueOnce(null);

    const fetch = useComputerStore.getState().fetchInstances();
    await useComputerStore.getState().deleteInstance('computer-a');
    resolveList([baseStatus]);
    await fetch;

    expect(useComputerStore.getState().instances).toEqual([]);
    expect(useComputerStore.getState().selectedInstanceId).toBeNull();
  });

  it('rejects late runtime events after a Computer is deleted', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({ snapshot_revision: 1 }),
    );
    mockedInvoke.mockResolvedValueOnce(null);

    await useComputerStore.getState().deleteInstance('computer-a');
    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({ lifecycle: 'shutdown', snapshot_revision: 2 }),
    );

    expect(useComputerStore.getState().instances).toEqual([]);
    expect(useRuntimeStore.getState().snapshots['computer-a']).toBeUndefined();
  });

  it('admits a higher incarnation when the same ID is created after deletion', async () => {
    const deletedRuntime = runtimeSnapshot({
      incarnation: 4,
      generation: 2,
      snapshot_revision: 3,
    });
    useComputerStore.setState({
      instances: [{
        id: 'computer-a',
        name: 'Old Computer',
        ...baseInstance,
        runtime: deletedRuntime,
      }],
      selectedInstanceId: 'computer-a',
    });
    useRuntimeStore.getState().receiveSnapshot('computer-a', deletedRuntime);
    mockedInvoke
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce({
        ...baseStatus,
        name: 'Recreated Computer',
        runtime: runtimeSnapshot({
          incarnation: 5,
          generation: 1,
          snapshot_revision: 1,
          lifecycle: 'created',
        }),
      });

    await useComputerStore.getState().deleteInstance('computer-a');
    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({
        incarnation: 4,
        generation: 2,
        snapshot_revision: 4,
        lifecycle: 'shutdown',
      }),
    );
    expect(useRuntimeStore.getState().snapshots['computer-a']).toBeUndefined();

    await useComputerStore.getState().createInstance({ name: 'Recreated Computer' });

    expect(useComputerStore.getState().instances).toHaveLength(1);
    expect(useComputerStore.getState().instances[0]).toMatchObject({
      id: 'computer-a',
      name: 'Recreated Computer',
      runtime: { incarnation: 5, generation: 1 },
    });
  });

  it('does not resurrect a deleted Computer from a delayed connection refresh', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    let resolveConnectionList!: (value: unknown) => void;
    mockedInvoke
      .mockResolvedValueOnce(undefined)
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveConnectionList = resolve;
      }))
      .mockResolvedValueOnce(null);

    const connect = useComputerStore.getState().connectSelectedTarget('computer-a');
    await vi.waitFor(() => expect(mockedInvoke).toHaveBeenCalledTimes(2));
    await useComputerStore.getState().deleteInstance('computer-a');
    resolveConnectionList([{
      ...baseStatus,
      runtime: runtimeSnapshot({ lifecycle: 'joined_office', snapshot_revision: 2 }),
    }]);
    await connect;

    expect(useComputerStore.getState().instances).toEqual([]);
  });

  it('starts and stops a Computer', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    mockedInvoke
      .mockResolvedValueOnce({
        ...baseStatus,
        running: true,
        runtime: runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 2 }),
      })
      .mockResolvedValueOnce({
        ...baseStatus,
        running: false,
        runtime: runtimeSnapshot({ lifecycle: 'shutdown', snapshot_revision: 3 }),
      });

    await useComputerStore.getState().startInstance('computer-a');
    expect(useComputerStore.getState().instances[0].status).toBe('running');

    await useComputerStore.getState().stopInstance('computer-a');
    expect(useComputerStore.getState().instances[0].status).toBe('stopped');
  });

  it('updates raw connection authority from start and stop status responses', async () => {
    const connectionContext = {
      profile_name: 'prod',
      url: 'https://smcp.example.com',
      office_id: 'office-a',
      computer_name: 'Computer A',
      connected_at: '2026-07-17T00:00:00Z',
      source_type: 'manager_robot',
    };
    useComputerStore.setState({
      instances: [{
        id: 'computer-a',
        name: 'A',
        ...baseInstance,
        clientConnectionPresent: false,
        clientConnectionContext: null,
      }],
      selectedInstanceId: 'computer-a',
    });
    mockedInvoke
      .mockResolvedValueOnce({
        ...baseStatus,
        running: true,
        connected: true,
        client_connection_present: true,
        connection_context: connectionContext,
        connection: connectionContext,
        runtime: runtimeSnapshot({ lifecycle: 'joined_office', snapshot_revision: 2 }),
      })
      .mockResolvedValueOnce({
        ...baseStatus,
        client_connection_present: false,
        connection_context: null,
        runtime: runtimeSnapshot({ lifecycle: 'shutdown', snapshot_revision: 3 }),
      });

    await useComputerStore.getState().startInstance('computer-a');
    expect(useComputerStore.getState().instances[0]).toMatchObject({
      clientConnectionPresent: true,
      clientConnectionContext: connectionContext,
      connectionStatus: 'connected',
      connectionProfile: 'prod',
    });

    await useComputerStore.getState().stopInstance('computer-a');
    expect(useComputerStore.getState().instances[0]).toMatchObject({
      clientConnectionPresent: false,
      clientConnectionContext: null,
      connectionStatus: 'disconnected',
    });
  });

  it('updates raw connection authority from restart and reload responses', async () => {
    const connectionContext = {
      profile_name: 'prod',
      url: 'https://smcp.example.com',
      office_id: 'office-a',
      computer_name: 'Computer A',
      connected_at: '2026-07-17T00:00:00Z',
    };
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    mockedInvoke
      .mockResolvedValueOnce({
        ...baseStatus,
        client_connection_present: true,
        connection_context: connectionContext,
        connection_revision: 1,
        runtime: runtimeSnapshot({ lifecycle: 'joined_office', snapshot_revision: 2 }),
      })
      .mockResolvedValueOnce({
        ...baseStatus,
        client_connection_present: false,
        connection_context: null,
        connection_revision: 2,
        runtime: runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 3 }),
      });

    await useComputerStore.getState().restartInstance('computer-a');
    expect(useComputerStore.getState().instances[0].clientConnectionPresent).toBe(true);

    await useComputerStore.getState().reloadRuntime('computer-a');
    expect(useComputerStore.getState().instances[0]).toMatchObject({
      clientConnectionPresent: false,
      clientConnectionContext: null,
      connectionStatus: 'disconnected',
    });
  });

  it('reprojects a newer runtime event when an action returns newer connection authority', async () => {
    const connectionContext = {
      profile_name: 'prod',
      url: 'https://smcp.example.com',
      office_id: 'office-a',
      computer_name: 'Computer A',
      connected_at: '2026-07-17T00:00:00Z',
    };
    mockedInvoke.mockResolvedValueOnce([{
      ...baseStatus,
      client_connection_present: false,
      connection_context: null,
      connection_revision: 1,
      runtime: runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 1 }),
    }]);
    await useComputerStore.getState().fetchInstances();
    useDashboardStore.setState({
      data: {
        computer_total: 1,
        computer_running: 1,
        computer_stopped: 0,
        computer_connected: 0,
        computers: [{
          id: 'computer-a',
          name: 'Computer A',
          running: true,
          connected: false,
          client_connection_present: false,
          connection_revision: 1,
          mcp_server_count: 1,
          runtime: runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 1 }),
        }],
        recent_logs: [],
        runtimes: [],
      },
    });
    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({ lifecycle: 'joined_office', snapshot_revision: 3 }),
    );
    mockedInvoke.mockResolvedValueOnce({
      ...baseStatus,
      connected: true,
      client_connection_present: true,
      connection_context: connectionContext,
      connection_revision: 2,
      runtime: runtimeSnapshot({ lifecycle: 'joined_office', snapshot_revision: 2 }),
    });

    await useComputerStore.getState().startInstance('computer-a');

    expect(useDashboardStore.getState().data).toMatchObject({
      computer_connected: 1,
      computers: [{ connected: true, connection_profile: 'prod' }],
    });
  });

  it('preserves structured missing-input failures for the UI', async () => {
    const error = {
      code: 'missing_secret',
      input_id: 'api-key',
      env_hint: 'A2C_INPUT_API_KEY',
      message: 'Required secret input is unresolved',
    };
    mockedInvoke.mockRejectedValueOnce(error);

    await expect(useComputerStore.getState().startInstance('computer-a')).rejects.toBe(error);
    expect(useComputerStore.getState().error).toBe('Required secret input is unresolved');
  });

  it('does not let an older list response overwrite a newer runtime event projection', async () => {
    const currentRuntime = runtimeSnapshot({
      generation: 2,
      snapshot_revision: 2,
      lifecycle: 'joined_office',
      config_revision: 4,
      capability_revision: 5,
    });
    let resolveList!: (value: unknown) => void;
    mockedInvoke.mockImplementationOnce(() => new Promise((resolve) => {
      resolveList = resolve;
    }));

    const fetch = useComputerStore.getState().fetchInstances();
    useRuntimeStore.getState().receiveSnapshot('computer-a', currentRuntime);
    resolveList([{
      ...baseStatus,
      runtime: runtimeSnapshot({
        generation: 2,
        snapshot_revision: 1,
        lifecycle: 'started',
        config_revision: 4,
        capability_revision: 5,
      }),
    }]);
    await fetch;

    expect(useComputerStore.getState().instances[0]).toMatchObject({
      runtime: currentRuntime,
      status: 'running',
      connectionStatus: 'disconnected',
    });
  });

  it('forgets authority when reconciliation removes a profile so the same ID can start a new incarnation', async () => {
    const oldRuntime = runtimeSnapshot({ incarnation: 4, generation: 3, snapshot_revision: 8 });
    useComputerStore.setState({
      instances: [{
        id: 'computer-a',
        name: 'Computer A',
        ...baseInstance,
        runtime: oldRuntime,
      }],
    });
    useRuntimeStore.getState().receiveSnapshot('computer-a', oldRuntime);
    mockedInvoke.mockResolvedValueOnce([]);

    await useComputerStore.getState().fetchInstances();
    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({
        incarnation: 4,
        generation: 3,
        snapshot_revision: 9,
        lifecycle: 'shutdown',
      }),
    );
    expect(useRuntimeStore.getState().snapshots['computer-a']).toBeUndefined();

    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({
        incarnation: 5,
        generation: 1,
        snapshot_revision: 1,
        lifecycle: 'created',
      }),
    );

    expect(useRuntimeStore.getState().snapshots['computer-a']).toMatchObject({
      incarnation: 5,
      generation: 1,
      snapshot_revision: 1,
    });
  });

  it('updates unified connection policy for a Computer', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    mockedInvoke.mockResolvedValueOnce({
      ...baseStatus,
      connection_policy: {
        target: { type: 'manager_robot', id: '11', robotAccountId: 1111 },
        auto_connect: true,
      },
    });

    await useComputerStore.getState().updateConnectionPolicy('computer-a', {
      target: { type: 'manager_robot', id: '11', robotAccountId: 1111 },
      auto_connect: true,
    });

    expect(mockedInvoke).toHaveBeenCalledWith('update_computer_connection_policy', {
      request: {
        id: 'computer-a',
        target: { type: 'manager_robot', id: '11', robotAccountId: 1111 },
        autoConnect: true,
      },
    });
    expect(useComputerStore.getState().instances[0].connectionPolicy).toEqual({
      target: { type: 'manager_robot', id: '11', robotAccountId: 1111 },
      auto_connect: true,
    });
  });

  it('connects and disconnects the selected target', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    mockedInvoke
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce([{
        ...baseStatus,
        connected: true,
        runtime: runtimeSnapshot({ lifecycle: 'joined_office', snapshot_revision: 2 }),
      }])
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce([{
        ...baseStatus,
        connected: false,
        runtime: runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 3 }),
      }]);

    await useComputerStore.getState().connectSelectedTarget('computer-a');
    expect(mockedInvoke).toHaveBeenCalledWith('connect_computer_connection_target', {
      id: 'computer-a',
    });
    expect(useComputerStore.getState().instances[0].connectionStatus).toBe('connected');

    await useComputerStore.getState().disconnectConnection('computer-a');
    expect(mockedInvoke).toHaveBeenCalledWith('disconnect_computer_connection_target', {
      id: 'computer-a',
    });
    expect(useComputerStore.getState().instances[0].connectionStatus).toBe('disconnected');
  });
});
