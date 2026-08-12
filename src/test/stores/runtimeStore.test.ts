import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useComputerStore } from '@/stores/computerStore';
import { useDashboardStore } from '@/stores/dashboardStore';
import { useDebugStore } from '@/stores/debugStore';
import { useConnectionStore } from '@/stores/connectionStore';
import { getClientConnectionAuthority } from '@/stores/connectionAuthority';
import { useMcpStore } from '@/stores/mcpStore';
import { useSdkConfigStore } from '@/stores/sdkConfigStore';
import {
  COMPUTER_RUNTIME_STATUS_EVENT,
  useRuntimeStore,
} from '@/stores/runtimeStore';
import { useSkillStore } from '@/stores/skillStore';
import { runtimeSnapshot } from '../helpers/store';

const mockedInvoke = vi.mocked(invoke);
const mockedListen = vi.mocked(listen);

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

describe('runtimeStore', () => {
  beforeEach(async () => {
    await useRuntimeStore.getState().dispose();
    useRuntimeStore.getState().reset();
    useComputerStore.getState().reset();
    useDashboardStore.getState().reset();
    useMcpStore.getState().reset();
    useSdkConfigStore.getState().reset();
    useDebugStore.getState().reset();
    useConnectionStore.getState().reset();
    useSkillStore.getState().reset();
    mockedInvoke.mockReset();
    mockedListen.mockReset();
    mockedListen.mockResolvedValue(() => {});
  });

  it('applies SDK snapshots to runtime consumers and refreshes active capabilities', async () => {
    const initialRuntime = runtimeSnapshot({ capability_revision: 1 });
    useComputerStore.setState({
      instances: [{
        id: 'computer-a',
        name: 'Computer A',
        status: 'running',
        connectionStatus: 'disconnected',
        connectionPolicy: { target: null, auto_connect: false },
        mcpServerCount: 1,
        runtime: initialRuntime,
      }],
    });
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
          mcp_server_count: 1,
          runtime: initialRuntime,
        }],
        recent_activity: [],
        runtimes: [],
      },
    });
    useMcpStore.setState({ activeInstanceId: 'computer-a' });
    useDebugStore.setState({ activeInstanceId: 'computer-a' });
    useSkillStore.setState({ activeInstanceId: 'computer-a' });
    const fetchServers = vi.spyOn(useMcpStore.getState(), 'fetchServers').mockResolvedValue();
    const fetchTools = vi.spyOn(useDebugStore.getState(), 'fetchTools').mockResolvedValue();
    const fetchSkills = vi.spyOn(useSkillStore.getState(), 'fetchSkills').mockResolvedValue();
    const fetchMarketplace = vi
      .spyOn(useSkillStore.getState(), 'fetchMarketplaceGovernance')
      .mockResolvedValue();

    const nextRuntime = runtimeSnapshot({
      snapshot_revision: 2,
      lifecycle: 'degraded',
      capability_revision: 2,
      mcp_servers: 4,
      active_mcp_servers: 3,
      tools: 12,
      skills: 5,
      degraded_reason: 'one MCP server failed',
    });
    useRuntimeStore.getState().receiveSnapshot('computer-a', nextRuntime);

    expect(useComputerStore.getState().instances[0]).toMatchObject({
      status: 'degraded',
      mcpServerCount: 4,
      runtime: nextRuntime,
    });
    expect(useDashboardStore.getState().data?.computers[0]).toMatchObject({
      running: true,
      mcp_server_count: 4,
      runtime: nextRuntime,
    });
    expect(fetchServers).toHaveBeenCalledWith('computer-a');
    expect(fetchTools).toHaveBeenCalledWith('computer-a');
    expect(fetchSkills).toHaveBeenCalledWith('computer-a');
    expect(fetchMarketplace).toHaveBeenCalledWith('computer-a');
  });

  it('refreshes active capabilities when a new incarnation restarts the counters', () => {
    useMcpStore.setState({ activeInstanceId: 'computer-a' });
    useDebugStore.setState({ activeInstanceId: 'computer-a' });
    useSkillStore.setState({ activeInstanceId: 'computer-a' });
    const fetchServers = vi.spyOn(useMcpStore.getState(), 'fetchServers').mockResolvedValue();
    const fetchTools = vi.spyOn(useDebugStore.getState(), 'fetchTools').mockResolvedValue();
    const fetchSkills = vi.spyOn(useSkillStore.getState(), 'fetchSkills').mockResolvedValue();
    const fetchMarketplace = vi
      .spyOn(useSkillStore.getState(), 'fetchMarketplaceGovernance')
      .mockResolvedValue();

    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({ incarnation: 7, generation: 1, capability_revision: 0 }),
    );
    vi.clearAllMocks();
    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({
        incarnation: 8,
        generation: 1,
        capability_revision: 0,
        lifecycle: 'created',
      }),
    );

    expect(fetchServers).toHaveBeenCalledWith('computer-a');
    expect(fetchTools).toHaveBeenCalledWith('computer-a');
    expect(fetchSkills).toHaveBeenCalledWith('computer-a');
    expect(fetchMarketplace).toHaveBeenCalledWith('computer-a');
  });

  it('routes config and capability revisions to only their dependent consumers', () => {
    useMcpStore.setState({ activeInstanceId: 'computer-a' });
    useSdkConfigStore.setState({ activeInstanceId: 'computer-a' });
    useDebugStore.setState({ activeInstanceId: 'computer-a' });
    useSkillStore.setState({ activeInstanceId: 'computer-a' });
    const fetchServers = vi.spyOn(useMcpStore.getState(), 'fetchServers').mockResolvedValue();
    const fetchConfig = vi.spyOn(useSdkConfigStore.getState(), 'fetchConfig').mockResolvedValue();
    const fetchTools = vi.spyOn(useDebugStore.getState(), 'fetchTools').mockResolvedValue();
    const fetchSkills = vi.spyOn(useSkillStore.getState(), 'fetchSkills').mockResolvedValue();
    const fetchMarketplace = vi
      .spyOn(useSkillStore.getState(), 'fetchMarketplaceGovernance')
      .mockResolvedValue();

    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({ config_revision: 1, capability_revision: 1 }),
    );
    fetchServers.mockClear();
    fetchConfig.mockClear();
    fetchTools.mockClear();
    fetchSkills.mockClear();
    fetchMarketplace.mockClear();

    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({
        snapshot_revision: 2,
        config_revision: 2,
        capability_revision: 1,
      }),
    );
    expect(fetchServers).toHaveBeenCalledOnce();
    expect(fetchConfig).toHaveBeenCalledOnce();
    expect(fetchTools).not.toHaveBeenCalled();
    expect(fetchSkills).not.toHaveBeenCalled();
    expect(fetchMarketplace).not.toHaveBeenCalled();

    fetchServers.mockClear();
    fetchConfig.mockClear();
    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({
        snapshot_revision: 3,
        config_revision: 2,
        capability_revision: 2,
      }),
    );
    expect(fetchServers).toHaveBeenCalledOnce();
    expect(fetchConfig).not.toHaveBeenCalled();
    expect(fetchTools).toHaveBeenCalledOnce();
    expect(fetchSkills).toHaveBeenCalledOnce();
    expect(fetchMarketplace).toHaveBeenCalledOnce();

    fetchServers.mockClear();
    fetchConfig.mockClear();
    fetchTools.mockClear();
    fetchSkills.mockClear();
    fetchMarketplace.mockClear();
    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({
        snapshot_revision: 4,
        lifecycle: 'degraded',
        config_revision: 2,
        capability_revision: 2,
      }),
    );
    expect(fetchServers).not.toHaveBeenCalled();
    expect(fetchConfig).not.toHaveBeenCalled();
    expect(fetchTools).not.toHaveBeenCalled();
    expect(fetchSkills).not.toHaveBeenCalled();
    expect(fetchMarketplace).not.toHaveBeenCalled();

    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({
        snapshot_revision: 5,
        lifecycle: 'degraded',
        config_revision: 3,
        capability_revision: 3,
      }),
    );
    expect(fetchServers).toHaveBeenCalledOnce();
    expect(fetchConfig).toHaveBeenCalledOnce();
    expect(fetchTools).toHaveBeenCalledOnce();
    expect(fetchSkills).toHaveBeenCalledOnce();
    expect(fetchMarketplace).toHaveBeenCalledOnce();
  });

  it('rejects events from an older handle generation even with larger revisions', () => {
    const current = runtimeSnapshot({ generation: 2, capability_revision: 3 });
    const stale = runtimeSnapshot({
      generation: 1,
      config_revision: 99,
      capability_revision: 99,
      lifecycle: 'error',
      last_error: 'stale handle failed',
      problems: [{
        id: 'sdk:1:runtime_error',
        source: 'sdk',
        operation: 'runtime',
        severity: 'error',
        affected_capabilities: [{ kind: 'runtime' }],
        occurred_at: '2026-07-29T02:00:00Z',
        current: true,
        message: 'sdk_runtime_error',
        recommended_actions: ['start_runtime'],
      }],
    });

    useRuntimeStore.getState().receiveSnapshot('computer-a', current);
    useRuntimeStore.getState().receiveSnapshot('computer-a', stale);

    expect(useRuntimeStore.getState().snapshots['computer-a']).toEqual(current);
  });

  it('clears recovered problems and does not carry them into a replacement generation', () => {
    const failed = runtimeSnapshot({
      generation: 2,
      snapshot_revision: 4,
      lifecycle: 'degraded',
      problems: [{
        id: 'mcp:2:server-a:start',
        source: 'mcp',
        operation: 'start',
        severity: 'degraded',
        affected_capabilities: [{ kind: 'mcp_server', bundle_id: 'server-a' }],
        occurred_at: '2026-07-29T02:00:00Z',
        current: true,
        message: 'mcp_start_failed',
        recommended_actions: ['restart_runtime'],
      }],
    });
    const recovered = runtimeSnapshot({
      generation: 2,
      snapshot_revision: 5,
      lifecycle: 'started',
      problems: [],
    });
    const replacement = runtimeSnapshot({
      generation: 3,
      snapshot_revision: 1,
      lifecycle: 'started',
      problems: [],
    });

    useRuntimeStore.getState().receiveSnapshot('computer-a', failed);
    useRuntimeStore.getState().receiveSnapshot('computer-a', recovered);
    expect(useRuntimeStore.getState().snapshots['computer-a'].problems).toEqual([]);
    useRuntimeStore.getState().receiveSnapshot('computer-a', replacement);
    expect(useRuntimeStore.getState().snapshots['computer-a'].problems).toEqual([]);
  });

  it('accepts a newer runtime incarnation even when its handle counters restart', () => {
    const retired = runtimeSnapshot({
      incarnation: 7,
      generation: 12,
      snapshot_revision: 40,
      lifecycle: 'shutdown',
    });
    const replacement = runtimeSnapshot({
      incarnation: 8,
      generation: 1,
      snapshot_revision: 1,
      lifecycle: 'created',
    });

    useRuntimeStore.getState().receiveSnapshot('computer-a', retired);
    useRuntimeStore.getState().receiveSnapshot('computer-a', replacement);

    expect(useRuntimeStore.getState().snapshots['computer-a']).toEqual(replacement);
  });

  it('preserves client connection authority across the reconnect lifecycle window', () => {
    const connectedRuntime = runtimeSnapshot({ lifecycle: 'joined_office' });
    const connectionContext = {
      profile_name: 'prod',
      url: 'https://smcp.example.com',
      office_id: 'office-a',
      computer_name: 'Computer A',
      connected_at: '2026-07-17T00:00:00Z',
    };
    useComputerStore.setState({
      instances: [{
        id: 'computer-a',
        name: 'Computer A',
        status: 'running',
        connectionStatus: 'connected',
        clientConnectionPresent: true,
        clientConnectionContext: connectionContext,
        connectionProfile: 'prod',
        connectionUrl: 'https://smcp.example.com',
        connectionPolicy: { target: null, auto_connect: false },
        mcpServerCount: 0,
        runtime: connectedRuntime,
      }],
    });
    useDashboardStore.setState({
      data: {
        computer_total: 1,
        computer_running: 1,
        computer_stopped: 0,
        computer_connected: 1,
        computers: [{
          id: 'computer-a',
          name: 'Computer A',
          running: true,
          connected: true,
          client_connection_present: true,
          connection_context: connectionContext,
          connection_profile: 'prod',
          mcp_server_count: 0,
          runtime: connectedRuntime,
        }],
        recent_activity: [],
        runtimes: [],
      },
    });
    useRuntimeStore.getState().receiveEvent({
      instance_id: 'computer-a',
      cause: {
        kind: 'client_connection_state_changed',
        revision: 1,
        status: 'connecting',
      },
      snapshot: runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 2 }),
      connection: {
        status: 'connecting',
        present: true,
        revision: 1,
        context: connectionContext,
        operation: 'reconnect',
        last_error: null,
        actions: {
          connect: { enabled: false, disabled_reason: 'transition_in_progress' },
          disconnect: { enabled: false, disabled_reason: 'transition_in_progress' },
        },
      },
    });
    expect(useComputerStore.getState().instances[0].connectionStatus).toBe('connecting');
    expect(useComputerStore.getState().instances[0].connectionProfile).toBe('prod');
    expect(useComputerStore.getState().instances[0].connectionUrl).toBe('https://smcp.example.com');
    expect(useDashboardStore.getState().data?.computer_connected).toBe(0);
    expect(useDashboardStore.getState().data?.computers[0].connection_profile).toBe('prod');

    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({ lifecycle: 'connected', snapshot_revision: 3 }),
    );
    expect(useComputerStore.getState().instances[0].connectionStatus).toBe('connecting');
    expect(useDashboardStore.getState().data?.computer_connected).toBe(0);
    expect(useDashboardStore.getState().data?.computers[0].connection_profile).toBe('prod');

    useRuntimeStore.getState().receiveEvent({
      instance_id: 'computer-a',
      cause: {
        kind: 'client_connection_state_changed',
        revision: 2,
        status: 'connected',
      },
      snapshot: runtimeSnapshot({ lifecycle: 'joined_office', snapshot_revision: 4 }),
      connection: {
        status: 'connected',
        present: true,
        revision: 2,
        context: connectionContext,
        operation: null,
        last_error: null,
        actions: {
          connect: { enabled: false, disabled_reason: 'already_connected' },
          disconnect: { enabled: true, disabled_reason: null },
        },
      },
    });
    expect(useComputerStore.getState().instances[0].connectionStatus).toBe('connected');
    expect(useDashboardStore.getState().data?.computer_connected).toBe(1);
    expect(useDashboardStore.getState().data?.computers[0].connection_profile).toBe('prod');
  });

  it('applies a newer connection authority even when its paired runtime snapshot is older', () => {
    const current = runtimeSnapshot({
      lifecycle: 'joined_office',
      snapshot_revision: 5,
    });
    useRuntimeStore.getState().receiveSnapshot('computer-a', current);

    const context = {
      profile_name: 'prod',
      url: 'https://smcp.example.com',
      office_id: 'office-a',
      computer_name: 'Computer A',
      connected_at: '2026-07-17T00:00:00Z',
      source_type: 'manager_robot',
      employee_id: 42,
    };
    useRuntimeStore.getState().receiveEvent({
      instance_id: 'computer-a',
      cause: {
        kind: 'client_connection_authority_changed',
        revision: 1,
        present: true,
      },
      snapshot: runtimeSnapshot({
        lifecycle: 'joined_office',
        snapshot_revision: 4,
      }),
      connection: { present: true, revision: 1, context },
    });

    expect(useRuntimeStore.getState().snapshots['computer-a']).toEqual(current);
    expect(useConnectionStore.getState().getStatus('computer-a')).toMatchObject({
      connected: true,
      profile_name: 'prod',
      employee_id: 42,
    });

    useRuntimeStore.getState().receiveEvent({
      instance_id: 'computer-a',
      cause: {
        kind: 'client_connection_authority_changed',
        revision: 2,
        present: false,
      },
      snapshot: runtimeSnapshot({
        lifecycle: 'started',
        snapshot_revision: 6,
      }),
      connection: { present: false, revision: 2, context: null },
    });
    expect(useConnectionStore.getState().getStatus('computer-a')).toMatchObject({
      status: 'disconnected',
      connected: false,
    });

    useRuntimeStore.getState().receiveEvent({
      instance_id: 'computer-a',
      cause: {
        kind: 'client_connection_authority_changed',
        revision: 1,
        present: true,
      },
      snapshot: runtimeSnapshot({
        lifecycle: 'joined_office',
        snapshot_revision: 5,
      }),
      connection: { present: true, revision: 1, context },
    });
    expect(useConnectionStore.getState().getStatus('computer-a')).toMatchObject({
      status: 'disconnected',
      connected: false,
    });
  });

  it('orders equal connection revisions by their paired runtime observation', () => {
    const connection = {
      status: 'disconnected' as const,
      present: false,
      revision: 7,
      context: null,
      operation: null,
      last_error: null,
      actions: {
        connect: { enabled: false, disabled_reason: 'connection_unavailable' as const },
        disconnect: { enabled: true, disabled_reason: null },
      },
    };
    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({ lifecycle: 'connected', snapshot_revision: 10 }),
      connection,
    );

    useRuntimeStore.getState().receiveEvent({
      instance_id: 'computer-a',
      cause: { kind: 'observation_advanced' },
      snapshot: runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 9 }),
      connection: {
        ...connection,
        actions: {
          connect: { enabled: true, disabled_reason: null },
          disconnect: { enabled: false, disabled_reason: 'not_connected' },
        },
      },
    });
    expect(useConnectionStore.getState().getStatus('computer-a').actions?.disconnect.enabled)
      .toBe(true);

    useRuntimeStore.getState().receiveEvent({
      instance_id: 'computer-a',
      cause: { kind: 'observation_advanced' },
      snapshot: runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 11 }),
      connection: {
        ...connection,
        actions: {
          connect: { enabled: true, disabled_reason: null },
          disconnect: { enabled: false, disabled_reason: 'not_connected' },
        },
      },
    });
    expect(useConnectionStore.getState().getStatus('computer-a').actions).toEqual({
      connect: { enabled: true, disabled_reason: null },
      disconnect: { enabled: false, disabled_reason: 'not_connected' },
    });
  });

  it('derives legacy disconnected capabilities from the paired runtime snapshot', () => {
    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({ lifecycle: 'connected' }),
      { present: false, revision: 1, context: null },
    );

    expect(useConnectionStore.getState().getStatus('computer-a').actions).toEqual({
      connect: { enabled: false, disabled_reason: 'connection_unavailable' },
      disconnect: { enabled: true, disabled_reason: null },
    });
  });

  it('does not reuse raw connection authority across runtime incarnations', () => {
    const retiredRuntime = runtimeSnapshot({
      incarnation: 7,
      lifecycle: 'joined_office',
    });
    useComputerStore.setState({
      instances: [{
        id: 'computer-a',
        name: 'Computer A',
        status: 'running',
        connectionStatus: 'connected',
        clientConnectionPresent: true,
        connectionPolicy: { target: null, auto_connect: false },
        mcpServerCount: 0,
        runtime: retiredRuntime,
      }],
    });
    useDashboardStore.setState({
      data: {
        computer_total: 1,
        computer_running: 1,
        computer_stopped: 0,
        computer_connected: 1,
        computers: [{
          id: 'computer-a',
          name: 'Computer A',
          running: true,
          connected: true,
          client_connection_present: true,
          mcp_server_count: 0,
          runtime: retiredRuntime,
        }],
        recent_activity: [],
        runtimes: [],
      },
    });

    useRuntimeStore.getState().receiveSnapshot('computer-a', runtimeSnapshot({
      incarnation: 8,
      lifecycle: 'joined_office',
    }));

    expect(useComputerStore.getState().instances[0]).toMatchObject({
      connectionStatus: 'disconnected',
      clientConnectionPresent: false,
    });
    expect(useDashboardStore.getState().data).toMatchObject({
      computer_connected: 0,
      computers: [{ connected: false, client_connection_present: false }],
    });
  });

  it('records only accepted runtime events in a bounded per-incarnation history', () => {
    for (let revision = 1; revision <= 52; revision += 1) {
      useRuntimeStore.getState().receiveEvent({
        instance_id: 'computer-a',
        cause: { kind: 'config_revision_bumped', revision },
        snapshot: runtimeSnapshot({
          snapshot_revision: revision,
          config_revision: revision,
        }),
        connection: { present: false, revision: 0, context: null },
      });
    }

    const history = useRuntimeStore.getState().eventsByInstance['computer-a'];
    expect(history).toHaveLength(50);
    expect(history[0]).toMatchObject({
      cause: { kind: 'config_revision_bumped', revision: 3 },
      snapshot: { snapshot_revision: 3 },
    });
    expect(history[history.length - 1]).toMatchObject({
      cause: { kind: 'config_revision_bumped', revision: 52 },
      snapshot: { snapshot_revision: 52 },
    });
    expect(history[0].received_at).toEqual(expect.any(String));

    useRuntimeStore.getState().receiveEvent({
      instance_id: 'computer-a',
      cause: { kind: 'config_revision_bumped', revision: 10 },
      snapshot: runtimeSnapshot({ snapshot_revision: 10, config_revision: 10 }),
      connection: { present: false, revision: 0, context: null },
    });
    expect(useRuntimeStore.getState().eventsByInstance['computer-a']).toHaveLength(50);

    useRuntimeStore.getState().receiveEvent({
      instance_id: 'computer-a',
      cause: { kind: 'handle_replaced', reason: 'restart' },
      snapshot: runtimeSnapshot({ incarnation: 2, snapshot_revision: 1 }),
      connection: { present: false, revision: 0, context: null },
    });
    expect(useRuntimeStore.getState().eventsByInstance['computer-a']).toMatchObject([
      {
        cause: { kind: 'handle_replaced', reason: 'restart' },
        snapshot: { incarnation: 2, snapshot_revision: 1 },
      },
    ]);

    useRuntimeStore.getState().evictSnapshot('computer-a', 2);
    expect(useRuntimeStore.getState().eventsByInstance['computer-a']).toBeUndefined();
  });

  it('rehydrates a lagged OAuth row without overwriting a newer event', async () => {
    const response = deferred<ReturnType<typeof useMcpStore.getState>['servers']>();
    mockedInvoke.mockReturnValueOnce(response.promise);
    useMcpStore.setState({
      activeInstanceId: 'computer-a',
      serversReady: true,
      servers: [{
        bundleId: 'protected',
        name: 'protected',
        activation_state: 'stopped',
        connection_state: 'disconnected',
        running: false,
        status_message: 'stopped',
        disabled: false,
        managedBy: { type: 'user' },
        oauth_status: { state: 'unauthorized' },
        oauth_interaction: 'interactive',
      }],
    });

    useRuntimeStore.getState().receiveEvent({
      instance_id: 'computer-a',
      cause: { kind: 'resync', skipped_events: 3 },
      snapshot: runtimeSnapshot({ snapshot_revision: 2 }),
      connection: { present: false, revision: 0, context: null },
    });
    expect(useMcpStore.getState().servers).toHaveLength(1);

    useRuntimeStore.getState().receiveEvent({
      instance_id: 'computer-a',
      cause: {
        kind: 'oauth_status_changed',
        bundle_id: 'protected',
        status: { state: 'authorized', scopes: ['tools.read'] },
      },
      snapshot: runtimeSnapshot({ snapshot_revision: 3 }),
      connection: { present: false, revision: 0, context: null },
    });
    response.resolve([{
      ...useMcpStore.getState().servers[0],
      oauth_status: { state: 'unauthorized' },
    }]);

    await vi.waitFor(() => {
      expect(useMcpStore.getState().servers[0].oauth_status).toEqual({
        state: 'authorized',
        scopes: ['tools.read'],
      });
    });
  });

  it('does not let reconciliation forget erase an explicit deletion tombstone', () => {
    useRuntimeStore.getState().receiveSnapshot('computer-a', runtimeSnapshot());
    useRuntimeStore.getState().evictSnapshot('computer-a', 1);
    useRuntimeStore.getState().forgetSnapshot('computer-a');
    useRuntimeStore.getState().receiveSnapshot(
      'computer-a',
      runtimeSnapshot({ generation: 2, snapshot_revision: 2 }),
    );

    expect(useRuntimeStore.getState().snapshots['computer-a']).toBeUndefined();
  });

  it('clears connection status with runtime authority and rejects late tombstoned events', () => {
    const connected = runtimeSnapshot({ lifecycle: 'joined_office' });
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
    useRuntimeStore.getState().receiveSnapshot('computer-a', connected, connection);
    expect(useConnectionStore.getState().statuses['computer-a']).toMatchObject({ connected: true });

    useRuntimeStore.getState().evictSnapshot('computer-a', connected.incarnation);
    expect(useConnectionStore.getState().statuses['computer-a']).toBeUndefined();
    expect(getClientConnectionAuthority('computer-a')).toBeUndefined();

    useRuntimeStore.getState().receiveEvent({
      instance_id: 'computer-a',
      cause: { kind: 'client_connection_authority_changed', revision: 2, present: true },
      snapshot: runtimeSnapshot({ snapshot_revision: 2 }),
      connection: { ...connection, revision: 2 },
    });
    expect(useConnectionStore.getState().statuses['computer-a']).toBeUndefined();
    expect(getClientConnectionAuthority('computer-a')).toBeUndefined();

    const replacement = runtimeSnapshot({
      incarnation: connected.incarnation + 1,
      lifecycle: 'joined_office',
    });
    useRuntimeStore.getState().receiveEvent({
      instance_id: 'computer-a',
      cause: { kind: 'client_connection_authority_changed', revision: 1, present: true },
      snapshot: replacement,
      connection: { ...connection, revision: 1 },
    });
    expect(useRuntimeStore.getState().snapshots['computer-a']).toBeUndefined();
    expect(useConnectionStore.getState().statuses['computer-a']).toBeUndefined();
    expect(getClientConnectionAuthority('computer-a')).toBeUndefined();

    useRuntimeStore.getState().receiveSnapshot('computer-a', replacement, {
      ...connection,
      revision: 1,
    }, { allowDeletedRediscovery: true });
    expect(useConnectionStore.getState().statuses['computer-a']).toMatchObject({ connected: true });

    useRuntimeStore.getState().forgetSnapshot('computer-a');
    expect(useConnectionStore.getState().statuses['computer-a']).toBeUndefined();
    expect(getClientConnectionAuthority('computer-a')).toBeUndefined();

    useRuntimeStore.getState().receiveEvent({
      instance_id: 'computer-a',
      cause: { kind: 'client_connection_authority_changed', revision: 2, present: true },
      snapshot: { ...replacement, snapshot_revision: 2 },
      connection: { ...connection, revision: 2 },
    });
    expect(useConnectionStore.getState().statuses['computer-a']).toBeUndefined();
    expect(getClientConnectionAuthority('computer-a')).toBeUndefined();
  });

  it('subscribes before enabling backend events and hydrates initial snapshots', async () => {
    const callOrder: string[] = [];
    let eventHandler: ((event: { payload: unknown }) => void) | undefined;
    mockedListen.mockImplementationOnce(async (_event, handler) => {
      callOrder.push('listen');
      eventHandler = handler as (event: { payload: unknown }) => void;
      return () => {};
    });
    const initialRuntime = runtimeSnapshot({ lifecycle: 'created' });
    mockedInvoke.mockImplementationOnce(async () => {
      callOrder.push('invoke');
      eventHandler?.({
        payload: {
          instance_id: 'computer-a',
          cause: { kind: 'lifecycle_changed', state: 'started' },
          snapshot: runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 2 }),
          connection: { present: false, revision: 0, context: null },
        },
      });
      return [{
        instance_id: 'computer-a',
        snapshot: initialRuntime,
        connection: { present: false, revision: 0, context: null },
      }];
    });

    await useRuntimeStore.getState().initialize();

    expect(callOrder).toEqual(['listen', 'invoke']);
    expect(mockedListen).toHaveBeenCalledWith(
      COMPUTER_RUNTIME_STATUS_EVENT,
      expect.any(Function),
    );
    expect(mockedInvoke).toHaveBeenCalledWith('enable_computer_runtime_events');
    expect(useRuntimeStore.getState().snapshots['computer-a']).toEqual(
      runtimeSnapshot({ lifecycle: 'started', snapshot_revision: 2 }),
    );
    expect(useRuntimeStore.getState().initialized).toBe(true);
  });

  it('hydrates connection authority with the initial runtime observation', async () => {
    const runtime = runtimeSnapshot({ lifecycle: 'joined_office' });
    mockedInvoke.mockResolvedValueOnce([{
      instance_id: 'computer-a',
      snapshot: runtime,
      connection: {
        present: true,
        revision: 3,
        context: {
          profile_name: 'manager:42',
          url: 'https://smcp.example.com',
          office_id: 'office-a',
          computer_name: 'Computer A',
          connected_at: '2026-07-17T00:00:00Z',
          source_type: 'manager_robot',
          employee_id: 42,
        },
      },
    }]);

    await useRuntimeStore.getState().initialize();

    expect(useConnectionStore.getState().getStatus('computer-a')).toMatchObject({
      connected: true,
      profile_name: 'manager:42',
      employee_id: 42,
    });
  });

  it('cancels stale StrictMode initialization without leaking its listener', async () => {
    const firstListen = deferred<() => void>();
    const secondListen = deferred<() => void>();
    const firstUnlisten = vi.fn();
    const secondUnlisten = vi.fn();
    mockedListen
      .mockReturnValueOnce(firstListen.promise)
      .mockReturnValueOnce(secondListen.promise);
    mockedInvoke.mockResolvedValue([]);

    const firstInitialize = useRuntimeStore.getState().initialize();
    await vi.waitFor(() => expect(mockedListen).toHaveBeenCalledTimes(1));
    await useRuntimeStore.getState().dispose();

    const secondInitialize = useRuntimeStore.getState().initialize();
    await vi.waitFor(() => expect(mockedListen).toHaveBeenCalledTimes(2));

    firstListen.resolve(firstUnlisten);
    await firstInitialize;
    expect(firstUnlisten).toHaveBeenCalledOnce();
    expect(mockedInvoke).not.toHaveBeenCalled();

    secondListen.resolve(secondUnlisten);
    await secondInitialize;
    expect(mockedInvoke).toHaveBeenCalledOnce();
    expect(secondUnlisten).not.toHaveBeenCalled();
    expect(useRuntimeStore.getState().initialized).toBe(true);

    await useRuntimeStore.getState().dispose();
    expect(secondUnlisten).toHaveBeenCalledOnce();
  });

  it('recovers a failed event bridge and explicitly resyncs authoritative snapshots', async () => {
    const recovered = runtimeSnapshot({ snapshot_revision: 7, lifecycle: 'started' });
    mockedListen.mockResolvedValue(() => {});
    mockedInvoke
      .mockRejectedValueOnce(new Error('event bridge unavailable'))
      .mockResolvedValueOnce([{ instance_id: 'computer-a', snapshot: recovered }])
      .mockResolvedValueOnce([{ instance_id: 'computer-a', snapshot: recovered }]);

    await expect(useRuntimeStore.getState().initialize()).rejects.toThrow(
      'event bridge unavailable',
    );
    expect(useRuntimeStore.getState()).toMatchObject({
      initialized: false,
      error: 'Error: event bridge unavailable',
    });

    await useRuntimeStore.getState().recover();

    expect(mockedInvoke).toHaveBeenNthCalledWith(2, 'enable_computer_runtime_events');
    expect(mockedInvoke).toHaveBeenNthCalledWith(3, 'get_computer_runtime_snapshots');
    expect(useRuntimeStore.getState()).toMatchObject({
      initialized: true,
      error: null,
      snapshots: { 'computer-a': recovered },
    });
  });
});
