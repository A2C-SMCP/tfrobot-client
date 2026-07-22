import { invoke } from '@tauri-apps/api/core';
import { useDesktopStore } from '@/stores/desktopStore';

const mockedInvoke = vi.mocked(invoke);
const instanceId = 'computer-a';

describe('desktopStore', () => {
  beforeEach(() => {
    useDesktopStore.getState().reset();
    mockedInvoke.mockReset();
  });

  describe('fetchDesktop', () => {
    it('populates windows', async () => {
      const mockWindows = [
        { uri: 'window://1', title: 'Test', server: 'desktop' },
      ];
      mockedInvoke.mockResolvedValueOnce(mockWindows);

      await useDesktopStore.getState().fetchDesktop(instanceId);

      expect(mockedInvoke).toHaveBeenCalledWith('get_desktop', {
        instanceId,
        uri: null,
      });
      expect(useDesktopStore.getState().windows).toEqual(mockWindows);
    });

    it('passes uri parameter', async () => {
      mockedInvoke.mockResolvedValueOnce([]);

      await useDesktopStore.getState().fetchDesktop(instanceId, 'window://test');

      expect(mockedInvoke).toHaveBeenCalledWith('get_desktop', {
        instanceId,
        uri: 'window://test',
      });
    });

    it('handles error', async () => {
      mockedInvoke.mockRejectedValueOnce('not available');

      await useDesktopStore.getState().fetchDesktop(instanceId);

      expect(useDesktopStore.getState().error).toBe('not available');
    });

    it('sets loading state during fetch', async () => {
      let resolveFn!: (value: unknown[]) => void;
      const promise = new Promise<unknown[]>((resolve) => {
        resolveFn = resolve;
      });
      mockedInvoke.mockReturnValueOnce(promise as any);

      const fetchPromise = useDesktopStore.getState().fetchDesktop(instanceId);

      // Loading should be true during fetch
      expect(useDesktopStore.getState().loading).toBe(true);

      // Resolve the promise to allow fetch to complete
      resolveFn([]);

      await fetchPromise;

      // Loading should be false after fetch
      expect(useDesktopStore.getState().loading).toBe(false);
    });
  });

  describe('fetchWindowDetail', () => {
    it('populates window detail on success', async () => {
      const mockDetail = {
        bundleId: 'desktop-bundle',
        uri: 'window://main',
        title: null,
        server: 'desktop-server',
        contents: [{ type: 'text', uri: 'window://main', text: 'Hello' }],
      };
      mockedInvoke.mockResolvedValueOnce(mockDetail);

      await useDesktopStore.getState().fetchWindowDetail(
        instanceId,
        'desktop-bundle',
        'window://main',
      );

      expect(mockedInvoke).toHaveBeenCalledWith('get_window_detail', {
        instanceId,
        bundleId: 'desktop-bundle',
        uri: 'window://main',
      });
      expect(useDesktopStore.getState().windowDetails['desktop-bundle:window://main']).toEqual(mockDetail);
    });

    it('sets detailErrors on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('detail not found');

      await useDesktopStore.getState().fetchWindowDetail(instanceId, 'bundle', 'window://fail');

      expect(useDesktopStore.getState().detailErrors['bundle:window://fail']).toBe('detail not found');
    });

    it('sets loadingDetails during fetch', async () => {
      let resolveFn!: (value: unknown) => void;
      const promise = new Promise((resolve) => {
        resolveFn = resolve;
      });
      mockedInvoke.mockReturnValueOnce(promise as any);

      const fetchPromise = useDesktopStore.getState().fetchWindowDetail(
        instanceId,
        'bundle',
        'window://loading',
      );

      expect(useDesktopStore.getState().loadingDetails['bundle:window://loading']).toBe(true);

      resolveFn({ bundleId: 'bundle', uri: 'window://loading', server: 'server', contents: [] });
      await fetchPromise;

      expect(useDesktopStore.getState().loadingDetails['bundle:window://loading']).toBeUndefined();
    });

    it('clears previous error for uri on success', async () => {
      // Set an existing error
      useDesktopStore.setState({
        detailErrors: { 'bundle:window://main': 'old error' },
      });

      const mockDetail = {
        bundleId: 'bundle',
        uri: 'window://main',
        server: 'server',
        contents: [],
      };
      mockedInvoke.mockResolvedValueOnce(mockDetail);

      await useDesktopStore.getState().fetchWindowDetail(instanceId, 'bundle', 'window://main');

      expect(useDesktopStore.getState().detailErrors['bundle:window://main']).toBeUndefined();
    });
  });

  describe('reset', () => {
    it('resets all state to initial values', () => {
      useDesktopStore.setState({
        windows: [{ bundleId: 'desktop', uri: 'window://1', title: 'Test', server: 'desktop' }],
        loading: true,
        error: 'some error',
        windowDetails: {
          'desktop:window://1': {
            bundleId: 'desktop',
            uri: 'window://1',
            server: 'desktop',
            contents: [],
          },
        },
        loadingDetails: { 'window://1': true },
        detailErrors: { 'window://1': 'error' },
      });

      useDesktopStore.getState().reset();

      expect(useDesktopStore.getState().windows).toEqual([]);
      expect(useDesktopStore.getState().loading).toBe(false);
      expect(useDesktopStore.getState().error).toBeNull();
      expect(useDesktopStore.getState().windowDetails).toEqual({});
      expect(useDesktopStore.getState().loadingDetails).toEqual({});
      expect(useDesktopStore.getState().detailErrors).toEqual({});
    });
  });
});
