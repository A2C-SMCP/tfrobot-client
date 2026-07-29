import { invoke } from '@tauri-apps/api/core';
import {
  desktopRuntimeKey,
  desktopWindowKey,
  selectDesktopInstance,
  useDesktopStore,
  type DesktopWindow,
} from '@/stores/desktopStore';
import { runtimeSnapshot } from '../helpers/store';

const mockedInvoke = vi.mocked(invoke);
const runtimeKey = desktopRuntimeKey(runtimeSnapshot({
  mcp_servers: 1,
  active_mcp_servers: 1,
}));

function desktopState(instanceId: string) {
  return selectDesktopInstance(useDesktopStore.getState(), instanceId);
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe('desktopStore', () => {
  beforeEach(() => {
    useDesktopStore.getState().reset();
    mockedInvoke.mockReset();
  });

  it('loads resource metadata for one Computer without reading details', async () => {
    const windows: DesktopWindow[] = [{
      bundleId: 'desktop-bundle',
      uri: 'window://1',
      title: 'Test',
      server: 'Desktop MCP',
      mime_type: 'text/plain',
    }];
    mockedInvoke.mockResolvedValueOnce({ status: 'unverified', windows });

    await useDesktopStore.getState().fetchDesktop('computer-a', runtimeKey);

    expect(mockedInvoke).toHaveBeenCalledTimes(1);
    expect(mockedInvoke).toHaveBeenCalledWith('get_desktop', {
      instanceId: 'computer-a',
      uri: null,
    });
    expect(desktopState('computer-a')).toMatchObject({
      windows,
      runtimeKey,
      enumerationStatus: 'unverified',
      loaded: true,
      loading: false,
      error: null,
    });
  });

  it('passes an optional URI filter', async () => {
    mockedInvoke.mockResolvedValueOnce({ status: 'unverified', windows: [] });

    await useDesktopStore.getState().fetchDesktop(
      'computer-a',
      runtimeKey,
      'window://test',
    );

    expect(mockedInvoke).toHaveBeenCalledWith('get_desktop', {
      instanceId: 'computer-a',
      uri: 'window://test',
    });
  });

  it('isolates list, loading and error state by Computer', async () => {
    const first = deferred<{ status: 'unverified'; windows: DesktopWindow[] }>();
    const second = deferred<{ status: 'unverified'; windows: DesktopWindow[] }>();
    mockedInvoke
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    const firstLoad = useDesktopStore.getState().fetchDesktop('computer-a', runtimeKey);
    const secondLoad = useDesktopStore.getState().fetchDesktop('computer-b', runtimeKey);

    expect(desktopState('computer-a').loading).toBe(true);
    expect(desktopState('computer-b').loading).toBe(true);

    second.resolve({
      status: 'unverified',
      windows: [{
        bundleId: 'bundle-b',
        uri: 'window://b',
        title: 'B',
        server: 'Server B',
      }],
    });
    await secondLoad;
    first.reject({ message: 'Computer A failed' });
    await firstLoad;

    expect(desktopState('computer-a')).toMatchObject({
      windows: [],
      loading: false,
      error: 'Computer A failed',
    });
    expect(desktopState('computer-b')).toMatchObject({
      windows: [expect.objectContaining({ uri: 'window://b' })],
      loaded: true,
      loading: false,
      error: null,
    });
  });

  it('ignores a stale list response for the same Computer', async () => {
    const stale = deferred<{ status: 'unverified'; windows: DesktopWindow[] }>();
    const latest = deferred<{ status: 'unverified'; windows: DesktopWindow[] }>();
    mockedInvoke
      .mockReturnValueOnce(stale.promise)
      .mockReturnValueOnce(latest.promise);

    const staleLoad = useDesktopStore.getState().fetchDesktop('computer-a', runtimeKey);
    const latestLoad = useDesktopStore.getState().fetchDesktop('computer-a', runtimeKey);
    latest.resolve({
      status: 'unverified',
      windows: [{
        bundleId: 'latest',
        uri: 'window://latest',
        title: 'Latest',
        server: 'Latest server',
      }],
    });
    await latestLoad;
    stale.resolve({
      status: 'unverified',
      windows: [{
        bundleId: 'stale',
        uri: 'window://stale',
        title: 'Stale',
        server: 'Stale server',
      }],
    });
    await staleLoad;

    expect(desktopState('computer-a').windows).toEqual([
      expect.objectContaining({ uri: 'window://latest' }),
    ]);
  });

  it('loads detail only into the requested Computer cache', async () => {
    const detail = {
      bundleId: 'desktop-bundle',
      uri: 'window://main',
      title: 'Main',
      server: 'Desktop MCP',
      contents: [{ type: 'text' as const, uri: 'window://main', text: 'Hello' }],
    };
    mockedInvoke.mockResolvedValueOnce(detail);

    await useDesktopStore.getState().fetchWindowDetail(
      'computer-a',
      runtimeKey,
      'desktop-bundle',
      'window://main',
    );

    const key = desktopWindowKey(detail);
    expect(mockedInvoke).toHaveBeenCalledWith('get_window_detail', {
      instanceId: 'computer-a',
      bundleId: 'desktop-bundle',
      uri: 'window://main',
    });
    expect(desktopState('computer-a').windowDetails[key]).toEqual(detail);
    expect(desktopState('computer-b').windowDetails).toEqual({});
  });

  it('tracks detail loading and structured errors per Computer and resource', async () => {
    const pending = deferred<never>();
    mockedInvoke.mockReturnValueOnce(pending.promise);
    const load = useDesktopStore.getState().fetchWindowDetail(
      'computer-a',
      runtimeKey,
      'bundle',
      'window://fail',
    );
    const key = desktopWindowKey({ bundleId: 'bundle', uri: 'window://fail' });

    expect(desktopState('computer-a').loadingDetails[key]).toBe(true);

    pending.reject({ code: 'runtime_error', message: 'Resource read failed' });
    await load;

    expect(desktopState('computer-a').loadingDetails[key]).toBeUndefined();
    expect(desktopState('computer-a').detailErrors[key]).toBe('Resource read failed');
  });

  it('refreshes only the requested Computer and clears only its stale details', async () => {
    const keyA = desktopWindowKey({ bundleId: 'a', uri: 'window://a' });
    const keyB = desktopWindowKey({ bundleId: 'b', uri: 'window://b' });
    useDesktopStore.setState({
      instances: {
        'computer-a': {
          ...desktopState('computer-a'),
          loaded: true,
          windowDetails: {
            [keyA]: {
              bundleId: 'a',
              uri: 'window://a',
              server: 'A',
              contents: [],
            },
          },
        },
        'computer-b': {
          ...desktopState('computer-b'),
          loaded: true,
          windowDetails: {
            [keyB]: {
              bundleId: 'b',
              uri: 'window://b',
              server: 'B',
              contents: [],
            },
          },
        },
      },
    });
    mockedInvoke.mockResolvedValueOnce({ status: 'unverified', windows: [] });

    await useDesktopStore.getState().fetchDesktop('computer-a', runtimeKey);

    expect(desktopState('computer-a').windowDetails).toEqual({});
    expect(desktopState('computer-b').windowDetails[keyB]).toBeDefined();
  });

  it('reset clears every Computer cache', () => {
    useDesktopStore.setState({
      instances: {
        'computer-a': {
          ...desktopState('computer-a'),
          loaded: true,
          error: 'old error',
        },
      },
    });

    useDesktopStore.getState().reset();

    expect(useDesktopStore.getState().instances).toEqual({});
    expect(desktopState('computer-a')).toMatchObject({
      windows: [],
      loaded: false,
      loading: false,
      error: null,
    });
  });

  it('invalidates list and detail caches when the Runtime identity changes', () => {
    const nextRuntimeKey = desktopRuntimeKey(runtimeSnapshot({
      generation: 2,
      capability_revision: 3,
      mcp_servers: 1,
      active_mcp_servers: 1,
    }));
    const key = desktopWindowKey({ bundleId: 'a', uri: 'window://a' });
    useDesktopStore.setState({
      instances: {
        'computer-a': {
          ...desktopState('computer-a'),
          runtimeKey,
          windows: [{
            bundleId: 'a',
            uri: 'window://a',
            title: 'A',
            server: 'A',
          }],
          enumerationStatus: 'unverified',
          loaded: true,
          windowDetails: {
            [key]: {
              bundleId: 'a',
              uri: 'window://a',
              server: 'A',
              contents: [],
            },
          },
        },
      },
    });

    useDesktopStore.getState().bindRuntime('computer-a', nextRuntimeKey);

    expect(desktopState('computer-a')).toMatchObject({
      runtimeKey: nextRuntimeKey,
      windows: [],
      enumerationStatus: null,
      loaded: false,
      windowDetails: {},
    });
  });

  it('rejects an old detail response after Runtime rebinding without ABA reuse', async () => {
    const stale = deferred<{
      bundleId: string;
      uri: string;
      server: string;
      contents: [];
    }>();
    mockedInvoke.mockReturnValueOnce(stale.promise);
    const staleLoad = useDesktopStore.getState().fetchWindowDetail(
      'computer-a',
      runtimeKey,
      'desktop-bundle',
      'window://main',
    );
    const nextRuntimeKey = desktopRuntimeKey(runtimeSnapshot({
      generation: 2,
      mcp_servers: 1,
      active_mcp_servers: 1,
    }));

    useDesktopStore.getState().bindRuntime('computer-a', nextRuntimeKey);
    stale.resolve({
      bundleId: 'desktop-bundle',
      uri: 'window://main',
      server: 'Desktop MCP',
      contents: [],
    });
    await staleLoad;

    expect(desktopState('computer-a').runtimeKey).toBe(nextRuntimeKey);
    expect(desktopState('computer-a').windowDetails).toEqual({});
  });
});
