import { invoke } from '@tauri-apps/api/core';
import { useComputerStore } from '@/stores/computerStore';

const mockedInvoke = vi.mocked(invoke);

const baseStatus = {
  id: 'computer-a',
  name: 'Computer A',
  description: 'Primary',
  running: false,
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
};

function resetStore() {
  useComputerStore.setState({
    instances: [],
    loading: false,
    error: null,
    selectedInstanceId: null,
  });
}

describe('computerStore', () => {
  beforeEach(() => {
    resetStore();
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

  it('duplicates a Computer with Robot binding and connection target options', async () => {
    mockedInvoke.mockResolvedValueOnce({ ...baseStatus, id: 'computer-copy', name: 'Computer A Copy' });

    await useComputerStore.getState().duplicateInstance({
      sourceId: 'computer-a',
      name: 'Computer A Copy',
      description: 'Copy',
      copyRobotBinding: true,
      connectionTargetId: 'target-a',
    });

    expect(mockedInvoke).toHaveBeenCalledWith('duplicate_computer_instance', {
      request: {
        sourceId: 'computer-a',
        name: 'Computer A Copy',
        description: 'Copy',
        copyRobotBinding: true,
        connectionTargetId: 'target-a',
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

  it('starts and stops a Computer', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    mockedInvoke
      .mockResolvedValueOnce({ ...baseStatus, running: true })
      .mockResolvedValueOnce({ ...baseStatus, running: false });

    await useComputerStore.getState().startInstance('computer-a');
    expect(useComputerStore.getState().instances[0].status).toBe('running');

    await useComputerStore.getState().stopInstance('computer-a');
    expect(useComputerStore.getState().instances[0].status).toBe('stopped');
  });

  it('updates unified connection policy for a Computer', async () => {
    useComputerStore.setState({
      instances: [{ id: 'computer-a', name: 'A', ...baseInstance }],
      selectedInstanceId: 'computer-a',
    });
    mockedInvoke.mockResolvedValueOnce({
      ...baseStatus,
      connection_policy: { target: { type: 'manager_robot', id: '11' }, auto_connect: true },
    });

    await useComputerStore.getState().updateConnectionPolicy('computer-a', {
      target: { type: 'manager_robot', id: '11' },
      auto_connect: true,
    });

    expect(mockedInvoke).toHaveBeenCalledWith('update_computer_connection_policy', {
      request: {
        id: 'computer-a',
        target: { type: 'manager_robot', id: '11' },
        autoConnect: true,
      },
    });
    expect(useComputerStore.getState().instances[0].connectionPolicy).toEqual({
      target: { type: 'manager_robot', id: '11' },
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
      .mockResolvedValueOnce([{ ...baseStatus, connected: true }])
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce([{ ...baseStatus, connected: false }]);

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
