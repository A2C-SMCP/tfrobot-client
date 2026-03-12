import { invoke } from '@tauri-apps/api/core';
import { useDesktopStore } from '@/stores/desktopStore';

const mockedInvoke = vi.mocked(invoke);

describe('desktopStore', () => {
  beforeEach(() => {
    useDesktopStore.setState({
      windows: [],
      loading: false,
      error: null,
    });
    mockedInvoke.mockReset();
  });

  describe('fetchDesktop', () => {
    it('populates windows', async () => {
      const mockWindows = [
        { uri: 'window://1', title: 'Test', server: 'desktop' },
      ];
      mockedInvoke.mockResolvedValueOnce(mockWindows);

      await useDesktopStore.getState().fetchDesktop();

      expect(mockedInvoke).toHaveBeenCalledWith('get_desktop', {
        size: null,
        uri: null,
      });
      expect(useDesktopStore.getState().windows).toEqual(mockWindows);
    });

    it('passes size and uri parameters', async () => {
      mockedInvoke.mockResolvedValueOnce([]);

      await useDesktopStore.getState().fetchDesktop('large', 'window://test');

      expect(mockedInvoke).toHaveBeenCalledWith('get_desktop', {
        size: 'large',
        uri: 'window://test',
      });
    });

    it('handles error', async () => {
      mockedInvoke.mockRejectedValueOnce('not available');

      await useDesktopStore.getState().fetchDesktop();

      expect(useDesktopStore.getState().error).toBe('not available');
    });

    it('sets loading state during fetch', async () => {
      let resolveFn!: (value: unknown[]) => void;
      const promise = new Promise<unknown[]>((resolve) => {
        resolveFn = resolve;
      });
      mockedInvoke.mockReturnValueOnce(promise as any);

      const fetchPromise = useDesktopStore.getState().fetchDesktop();

      // Loading should be true during fetch
      expect(useDesktopStore.getState().loading).toBe(true);

      // Resolve the promise to allow fetch to complete
      resolveFn([]);

      await fetchPromise;

      // Loading should be false after fetch
      expect(useDesktopStore.getState().loading).toBe(false);
    });
  });

  describe('reset', () => {
    it('resets all state to initial values', () => {
      useDesktopStore.setState({
        windows: [{ uri: 'window://1', title: 'Test', server: 'desktop' }],
        loading: true,
        error: 'some error',
      });

      useDesktopStore.getState().reset();

      expect(useDesktopStore.getState().windows).toEqual([]);
      expect(useDesktopStore.getState().loading).toBe(false);
      expect(useDesktopStore.getState().error).toBeNull();
    });
  });
});
