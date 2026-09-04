import { fireEvent, render, screen, waitFor } from '../helpers/render';
import { invoke } from '@tauri-apps/api/core';
import { check } from '@tauri-apps/plugin-updater';
import { AboutSection } from '@/components/Settings/AboutSection';

vi.mock('@/stores/settingsStore', () => ({
  useSettingsStore: () => ({
    appInfo: { version: '0.2.3', smcp_computer_version: '0.4.1' },
    fetchAppInfo: vi.fn().mockResolvedValue(undefined),
  }),
}));

describe('AboutSection update activity', () => {
  beforeEach(() => vi.clearAllMocks());

  it('records update check failures without exposing them as successful checks', async () => {
    vi.mocked(check).mockRejectedValueOnce(new Error('network unavailable'));

    render(<AboutSection />);
    fireEvent.click(screen.getByRole('button', { name: 'Check for Updates' }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith(
      'record_application_update_activity',
      expect.objectContaining({
        activity: 'check_failed',
        targetVersion: null,
        error: 'Error: network unavailable',
      }),
    ));
  });

  it('records one correlated start and success around installation', async () => {
    const downloadAndInstall = vi.fn().mockResolvedValue(undefined);
    vi.mocked(check).mockResolvedValueOnce({
      version: '0.3.0',
      downloadAndInstall,
    } as never);

    render(<AboutSection />);
    fireEvent.click(screen.getByRole('button', { name: 'Check for Updates' }));
    fireEvent.click(await screen.findByRole('button', { name: 'OK' }));

    await waitFor(() => expect(downloadAndInstall).toHaveBeenCalledOnce());
    const updateCalls = vi.mocked(invoke).mock.calls.filter(
      ([command]) => command === 'record_application_update_activity',
    );
    expect(updateCalls).toHaveLength(2);
    expect(updateCalls[0][1]).toEqual(expect.objectContaining({
      activity: 'install_started',
      targetVersion: '0.3.0',
    }));
    expect(updateCalls[1][1]).toEqual(expect.objectContaining({
      activity: 'install_succeeded',
      targetVersion: '0.3.0',
    }));
    expect((updateCalls[0][1] as { correlationId: string }).correlationId)
      .toBe((updateCalls[1][1] as { correlationId: string }).correlationId);
  });

  it('records an installation failure with the same correlation id', async () => {
    const downloadAndInstall = vi.fn().mockRejectedValue(new Error('token=private-update-token'));
    vi.mocked(check).mockResolvedValueOnce({
      version: '0.3.0',
      downloadAndInstall,
    } as never);

    render(<AboutSection />);
    fireEvent.click(screen.getByRole('button', { name: 'Check for Updates' }));
    fireEvent.click(await screen.findByRole('button', { name: 'OK' }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith(
      'record_application_update_activity',
      expect.objectContaining({
        activity: 'install_failed',
        targetVersion: '0.3.0',
        error: 'Error: token=private-update-token',
      }),
    ));
    const updateCalls = vi.mocked(invoke).mock.calls.filter(
      ([command]) => command === 'record_application_update_activity',
    );
    expect(updateCalls).toHaveLength(2);
    expect((updateCalls[0][1] as { correlationId: string }).correlationId)
      .toBe((updateCalls[1][1] as { correlationId: string }).correlationId);
  });
});
