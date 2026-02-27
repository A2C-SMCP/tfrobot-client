import { invoke } from '@tauri-apps/api/core';
import { useDesktopStore } from '@/stores/desktopStore';

const mockedInvoke = vi.mocked(invoke);

describe('desktopStore', () => {
  beforeEach(() => {
    useDesktopStore.setState({ windows: [], loading: false, error: null });
    mockedInvoke.mockReset();
  });

  it('fetchDesktop populates windows', async () => {
    const mockWindows = [
      { uri: 'window://1', title: 'Test', server: 'desktop' },
    ];
    mockedInvoke.mockResolvedValueOnce(mockWindows);

    await useDesktopStore.getState().fetchDesktop();

    expect(mockedInvoke).toHaveBeenCalledWith('get_desktop', { size: null, uri: null });
    expect(useDesktopStore.getState().windows).toEqual(mockWindows);
  });

  it('fetchDesktop handles error', async () => {
    mockedInvoke.mockRejectedValueOnce('not available');

    await useDesktopStore.getState().fetchDesktop();

    expect(useDesktopStore.getState().error).toBe('not available');
  });
});
