import { invoke } from '@tauri-apps/api/core';
import { useComputerStore } from '@/stores/computerStore';
import { getClientConnectionAuthority } from '@/stores/connectionAuthority';
import { useDashboardStore } from '@/stores/dashboardStore';
import { useConnectionStore } from '@/stores/connectionStore';
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
  status: 'not_running' as const,
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
    pendingMutationCount: 0,
    listRequestId: 0,
    mutationEpoch: 0,
    mutationCompletionRevision: 0,
    deletionRevision: 0,
    profileMutationVersions: {},
    connectionMetadataRequestIds: {},
  });
}

describe('computerStore', () => {
  beforeEach(() => {
    useRuntimeStore.getState().reset();
    resetStore();
    useDashboardStore.getState().reset();
    useConnectionStore.getState().reset();
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

  it('does not let a list started during a profile mutation undo its committed result', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    let resolveProfile!: (value: unknown) => void;
    let resolveList!: (value: unknown) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveProfile = resolve;
      }))
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveList = resolve;
      }));

    const profile = useComputerStore.getState().updateConnectionPolicy('computer-a', {
      target: { type: 'manual_smcp', id: 'new-target' },
      auto_connect: true,
    });
    const list = useComputerStore.getState().fetchInstances();
    resolveProfile({
      ...baseStatus,
      connection_policy: {
        target: { type: 'manual_smcp', id: 'new-target' },
        auto_connect: true,
      },
    });
    await profile;
    resolveList([{
      ...baseStatus,
      connection_policy: { target: null, auto_connect: false },
    }]);
    await list;

    expect(useComputerStore.getState().instances[0].connectionPolicy).toEqual({
      target: { type: 'manual_smcp', id: 'new-target' },
      auto_connect: true,
    });
    expect(useComputerStore.getState().error).toBeNull();
  });

  it('does not let a list started during metadata reconciliation overwrite or report stale data', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    let resolveMetadata!: (value: unknown) => void;
    let rejectList!: (reason: unknown) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveMetadata = resolve;
      }))
      .mockImplementationOnce(() => new Promise((_resolve, reject) => {
        rejectList = reject;
      }));

    const metadata = useComputerStore.getState().reconcileConnectionMetadata('computer-a');
    const list = useComputerStore.getState().fetchInstances();
    resolveMetadata([{
      ...baseStatus,
      robot_binding: { employee_id: 11, robot_name: 'New Robot' },
      connection_policy: {
        target: { type: 'manager_robot', id: '11' },
        auto_connect: true,
      },
    }]);
    await metadata;
    rejectList('stale list failure');
    await list;

    expect(useComputerStore.getState()).toMatchObject({
      error: null,
      instances: [{
        robotName: 'New Robot',
        connectionPolicy: {
          target: { type: 'manager_robot', id: '11' },
          auto_connect: true,
        },
      }],
    });
  });

  it('keeps a newer metadata reconciliation current when an older runtime mutation completes', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    let resolveStart!: (value: unknown) => void;
    let resolveMetadata!: (value: unknown) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveStart = resolve;
      }))
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveMetadata = resolve;
      }));

    const olderStart = useComputerStore.getState().startInstance('computer-a');
    const metadata = useComputerStore.getState().reconcileConnectionMetadata('computer-a');
    resolveStart({
      ...baseStatus,
      running: true,
      runtime: runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 2 }),
    });
    await olderStart;
    resolveMetadata([{
      ...baseStatus,
      robot_binding: { employee_id: 11, robot_name: 'New Robot' },
      connection_policy: {
        target: { type: 'manager_robot', id: '11' },
        auto_connect: true,
      },
    }]);
    await metadata;

    expect(useComputerStore.getState().instances[0]).toMatchObject({
      robotName: 'New Robot',
      connectionPolicy: {
        target: { type: 'manager_robot', id: '11' },
        auto_connect: true,
      },
    });
  });

  it('does not let an older reconciliation completion swallow the newest failure', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    let resolveOlder!: (value: unknown) => void;
    let rejectNewest!: (reason: unknown) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveOlder = resolve;
      }))
      .mockImplementationOnce(() => new Promise((_resolve, reject) => {
        rejectNewest = reject;
      }));

    const older = useComputerStore.getState().reconcileConnectionMetadata('computer-a');
    const newest = useComputerStore.getState().reconcileConnectionMetadata('computer-a');
    resolveOlder([baseStatus]);
    await older;
    rejectNewest('newest metadata failure');
    await expect(newest).rejects.toThrow(
      'Connection succeeded, but refreshing its saved binding and policy failed: newest metadata failure',
    );
    expect(useComputerStore.getState().error).toBe(
      'Connection succeeded, but refreshing its saved binding and policy failed: newest metadata failure',
    );
  });

  it('does not let a list started during create remove the committed instance', async () => {
    let resolveCreate!: (value: unknown) => void;
    let resolveList!: (value: unknown) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveCreate = resolve;
      }))
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveList = resolve;
      }));

    const creation = useComputerStore.getState().createInstance({ name: 'Created' });
    const list = useComputerStore.getState().fetchInstances();
    resolveCreate({ ...baseStatus, id: 'computer-created', name: 'Created' });
    await creation;
    resolveList([]);
    await list;

    expect(useComputerStore.getState().instances).toMatchObject([
      { id: 'computer-created', name: 'Created' },
    ]);
  });

  it('does not let an older full list overwrite reconciled connection metadata', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    let resolveOldList!: (value: unknown) => void;
    let resolveMetadata!: (value: unknown) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveOldList = resolve;
      }))
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveMetadata = resolve;
      }));

    const oldList = useComputerStore.getState().fetchInstances();
    const metadata = useComputerStore.getState().reconcileConnectionMetadata('computer-a');
    resolveMetadata([{
      ...baseStatus,
      robot_binding: { employee_id: 11, robot_name: 'New Robot' },
      connection_policy: {
        target: { type: 'manager_robot', id: '11' },
        auto_connect: true,
      },
    }]);
    await metadata;
    expect(useComputerStore.getState().instances[0]).toMatchObject({
      robotName: 'New Robot',
      connectionPolicy: {
        target: { type: 'manager_robot', id: '11' },
        auto_connect: true,
      },
    });

    resolveOldList([{
      ...baseStatus,
      robot_binding: null,
      connection_policy: { target: null, auto_connect: false },
    }]);
    await oldList;
    expect(useComputerStore.getState().instances[0]).toMatchObject({
      robotName: 'New Robot',
      connectionPolicy: {
        target: { type: 'manager_robot', id: '11' },
        auto_connect: true,
      },
    });
  });

  it('ignores an older full-list failure after connection metadata is reconciled', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    let rejectOldList!: (reason: unknown) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((_resolve, reject) => {
        rejectOldList = reject;
      }))
      .mockResolvedValueOnce([{
        ...baseStatus,
        robot_binding: { employee_id: 11, robot_name: 'New Robot' },
        connection_policy: {
          target: { type: 'manager_robot', id: '11' },
          auto_connect: true,
        },
      }]);

    const oldList = useComputerStore.getState().fetchInstances();
    await useComputerStore.getState().reconcileConnectionMetadata('computer-a');
    rejectOldList('stale list failure');
    await oldList;

    expect(useComputerStore.getState()).toMatchObject({
      error: null,
      loading: false,
      instances: [{
        robotName: 'New Robot',
        connectionPolicy: {
          target: { type: 'manager_robot', id: '11' },
          auto_connect: true,
        },
      }],
    });
  });

  it('does not let an older profile success overwrite reconciled connection metadata', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    let resolveOldProfile!: (value: unknown) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveOldProfile = resolve;
      }))
      .mockResolvedValueOnce([{
        ...baseStatus,
        robot_binding: { employee_id: 11, robot_name: 'New Robot' },
        connection_policy: {
          target: { type: 'manager_robot', id: '11' },
          auto_connect: true,
        },
      }]);

    const oldProfile = useComputerStore.getState().updateConnectionPolicy('computer-a', {
      target: { type: 'manual_smcp', id: 'old-target' },
      auto_connect: false,
    });
    await useComputerStore.getState().reconcileConnectionMetadata('computer-a');
    resolveOldProfile({
      ...baseStatus,
      robot_binding: null,
      connection_policy: {
        target: { type: 'manual_smcp', id: 'old-target' },
        auto_connect: false,
      },
    });
    await oldProfile;

    expect(useComputerStore.getState()).toMatchObject({
      error: null,
      loading: false,
      instances: [{
        robotName: 'New Robot',
        connectionPolicy: {
          target: { type: 'manager_robot', id: '11' },
          auto_connect: true,
        },
      }],
    });
  });

  it('ignores an older profile failure after connection metadata is reconciled', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    let rejectOldProfile!: (reason: unknown) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((_resolve, reject) => {
        rejectOldProfile = reject;
      }))
      .mockResolvedValueOnce([{
        ...baseStatus,
        robot_binding: { employee_id: 11, robot_name: 'New Robot' },
        connection_policy: {
          target: { type: 'manager_robot', id: '11' },
          auto_connect: true,
        },
      }]);

    const oldProfile = useComputerStore.getState().updateSkillHome(
      'computer-a',
      '/old/skill/home',
    );
    await useComputerStore.getState().reconcileConnectionMetadata('computer-a');
    rejectOldProfile('stale profile failure');
    await expect(oldProfile).rejects.toBe('stale profile failure');

    expect(useComputerStore.getState()).toMatchObject({
      error: null,
      loading: false,
      instances: [{
        robotName: 'New Robot',
        connectionPolicy: {
          target: { type: 'manager_robot', id: '11' },
          auto_connect: true,
        },
      }],
    });
  });

  it('ignores a stale metadata reconciliation failure after a newer profile mutation', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    let rejectMetadata!: (reason: unknown) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((_resolve, reject) => {
        rejectMetadata = reject;
      }))
      .mockResolvedValueOnce({
        ...baseStatus,
        connection_policy: {
          target: { type: 'manual_smcp', id: 'manual-a' },
          auto_connect: true,
        },
      });

    const staleMetadata = useComputerStore
      .getState()
      .reconcileConnectionMetadata('computer-a');
    await useComputerStore.getState().updateConnectionPolicy('computer-a', {
      target: { type: 'manual_smcp', id: 'manual-a' },
      auto_connect: true,
    });
    rejectMetadata('stale metadata failure');
    await staleMetadata;

    expect(useComputerStore.getState().error).toBeNull();
    expect(useComputerStore.getState().instances[0].connectionPolicy).toEqual({
      target: { type: 'manual_smcp', id: 'manual-a' },
      auto_connect: true,
    });
  });

  it('surfaces a current post-connect metadata reconciliation failure', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    mockedInvoke.mockRejectedValueOnce('metadata unavailable');

    await expect(
      useComputerStore.getState().reconcileConnectionMetadata('computer-a'),
    ).rejects.toThrow(
      'Connection succeeded, but refreshing its saved binding and policy failed: metadata unavailable',
    );

    expect(useComputerStore.getState()).toMatchObject({
      loading: false,
      error: 'Connection succeeded, but refreshing its saved binding and policy failed: metadata unavailable',
    });
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

  it('does not trust a list started while backend deletion is still in flight', async () => {
    const knownRuntime = runtimeSnapshot({
      incarnation: 4,
      lifecycle: 'joined_office',
    });
    const connection = {
      present: true,
      revision: 1,
      context: {
        profile_name: 'prod',
        url: 'https://smcp.example.com',
        office_id: 'office-a',
        computer_name: 'Computer A',
        connected_at: '2026-07-17T00:00:00Z',
        source_type: 'manual_smcp',
      },
    };
    useComputerStore.setState({
      instances: [{
        id: 'computer-a',
        name: 'A',
        ...baseInstance,
        runtime: knownRuntime,
      }],
      selectedInstanceId: 'computer-a',
    });
    useRuntimeStore.getState().receiveSnapshot('computer-a', knownRuntime, connection);

    let resolveDelete!: (value: unknown) => void;
    let resolveList!: (value: unknown) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveDelete = resolve;
      }))
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveList = resolve;
      }));

    const deletion = useComputerStore.getState().deleteInstance('computer-a');
    const list = useComputerStore.getState().fetchInstances();
    resolveList([{
      ...baseStatus,
      runtime: runtimeSnapshot({ incarnation: 5, lifecycle: 'joined_office' }),
      connected: true,
      client_connection_present: true,
      connection_revision: 2,
      connection_context: connection.context,
    }]);
    resolveDelete(null);
    await Promise.all([deletion, list]);

    expect(useComputerStore.getState().instances).toEqual([]);
    expect(useRuntimeStore.getState().snapshots['computer-a']).toBeUndefined();
    expect(useConnectionStore.getState().statuses['computer-a']).toBeUndefined();
    expect(getClientConnectionAuthority('computer-a')).toBeUndefined();
  });

  it('keeps mutation loading while a list succeeds during pending deletion', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    let resolveDelete!: (value: unknown) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveDelete = resolve;
      }))
      .mockResolvedValueOnce([baseStatus]);

    const deletion = useComputerStore.getState().deleteInstance('computer-a');
    await useComputerStore.getState().fetchInstances();

    expect(useComputerStore.getState()).toMatchObject({
      loading: true,
      pendingMutationCount: 1,
      error: null,
    });

    resolveDelete(null);
    await deletion;
    expect(useComputerStore.getState()).toMatchObject({
      loading: false,
      pendingMutationCount: 0,
      error: null,
    });
  });

  it('does not retain a list failure that occurs during pending deletion', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    let resolveDelete!: (value: unknown) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveDelete = resolve;
      }))
      .mockRejectedValueOnce('list failed while deleting');

    const deletion = useComputerStore.getState().deleteInstance('computer-a');
    await useComputerStore.getState().fetchInstances();
    expect(useComputerStore.getState()).toMatchObject({
      loading: true,
      pendingMutationCount: 1,
      error: null,
    });

    resolveDelete(null);
    await deletion;
    expect(useComputerStore.getState()).toMatchObject({
      loading: false,
      pendingMutationCount: 0,
      error: null,
    });
  });

  it('does not let a successful delete clear another concurrent mutation error', async () => {
    useComputerStore.setState({
      instances: [
        { id: 'computer-a', name: 'A', ...baseInstance },
        { id: 'computer-b', name: 'B', ...baseInstance },
      ],
      selectedInstanceId: 'computer-a',
    });
    let rejectStart!: (reason: unknown) => void;
    let resolveDelete!: (value: unknown) => void;
    mockedInvoke
      .mockImplementationOnce(() => new Promise((_resolve, reject) => {
        rejectStart = reject;
      }))
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveDelete = resolve;
      }));

    const start = useComputerStore.getState().startInstance('computer-a');
    const deletion = useComputerStore.getState().deleteInstance('computer-b');
    rejectStart('start failed');
    await expect(start).rejects.toBe('start failed');
    resolveDelete(null);
    await deletion;

    expect(useComputerStore.getState()).toMatchObject({
      loading: false,
      pendingMutationCount: 0,
      error: 'start failed',
    });
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
      runtimeSnapshot({ incarnation: 2, lifecycle: 'shutdown', snapshot_revision: 2 }),
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
    expect(useComputerStore.getState().instances[0].status).toBe('not_running');
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

  it('updates raw connection authority from restart responses', async () => {
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

    await useComputerStore.getState().restartInstance('computer-a');
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
      env_hint: 'A2C_SMCP_api_key',
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
        client_connection_present: true,
        connection_revision: 4,
        connection_context: {
          profile_name: 'manager:11',
          url: 'https://smcp.example.com',
          office_id: 'office-a',
          computer_name: 'A',
          connected_at: '2026-07-17T00:00:00Z',
          source_type: 'manager_robot',
          employee_id: 11,
        },
        robot_binding: { employee_id: 11, robot_name: 'Robot 11' },
        connection_policy: {
          target: { type: 'manager_robot', id: '11' },
          auto_connect: false,
        },
        runtime: runtimeSnapshot({ lifecycle: 'joined_office', snapshot_revision: 2 }),
      }])
      .mockResolvedValueOnce(undefined);

    await useComputerStore.getState().connectSelectedTarget('computer-a');
    expect(mockedInvoke).toHaveBeenCalledWith('connect_computer_connection_target', {
      id: 'computer-a',
    });
    expect(useComputerStore.getState().instances[0]).toMatchObject({
      connectionStatus: 'disconnected',
      robotName: 'Robot 11',
      connectionPolicy: { target: { type: 'manager_robot', id: '11' } },
    });

    useRuntimeStore.getState().receiveEvent({
      instance_id: 'computer-a',
      cause: { kind: 'client_connection_authority_changed', revision: 4, present: true },
      snapshot: runtimeSnapshot({ lifecycle: 'joined_office', snapshot_revision: 2 }),
      connection: {
        present: true,
        revision: 4,
        context: {
          profile_name: 'manager:11',
          url: 'https://smcp.example.com',
          office_id: 'office-a',
          computer_name: 'A',
          connected_at: '2026-07-17T00:00:00Z',
          source_type: 'manager_robot',
          employee_id: 11,
        },
      },
    });
    expect(useComputerStore.getState().instances[0].connectionStatus).toBe('connected');

    await useComputerStore.getState().disconnectConnection('computer-a');
    expect(mockedInvoke).toHaveBeenCalledWith('disconnect_computer_connection_target', {
      id: 'computer-a',
    });
    expect(useComputerStore.getState().instances[0].connectionStatus).toBe('connected');

    useRuntimeStore.getState().receiveEvent({
      instance_id: 'computer-a',
      cause: { kind: 'client_connection_authority_changed', revision: 5, present: false },
      snapshot: runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 3 }),
      connection: { present: false, revision: 5, context: null },
    });
    expect(useComputerStore.getState().instances[0].connectionStatus).toBe('disconnected');
    expect(mockedInvoke).toHaveBeenCalledTimes(3);
  });
});
