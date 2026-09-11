import { PageActivity } from '@/components/Navigation/PageActivity';
import { act, fireEvent, render, screen, waitFor } from '../helpers/render';
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

  it('releases a late update result without opening a confirmation after leaving About', async () => {
    let finish!: (value: Awaited<ReturnType<typeof check>>) => void;
    vi.mocked(check).mockReturnValueOnce(new Promise((resolve) => { finish = resolve; }));
    const close = vi.fn().mockResolvedValue(undefined);
    const install = vi.fn();
    const tree = (active: boolean) => <PageActivity active={active}><AboutSection /></PageActivity>;
    const view = render(tree(true));
    fireEvent.click(screen.getByRole('button', { name: 'Check for Updates' }));
    view.rerender(tree(false));
    await act(async () => { finish({ version: '0.3.0', close, downloadAndInstall: install } as never); });
    view.rerender(tree(true));
    expect(screen.queryByRole('button', { name: 'OK' })).not.toBeInTheDocument();
    expect(close).toHaveBeenCalledOnce();
    expect(install).not.toHaveBeenCalled();
  });

  it('dismisses an unconfirmed update when switching an internal Settings tab', async () => {
    const close = vi.fn().mockResolvedValue(undefined);
    const install = vi.fn();
    vi.mocked(check).mockResolvedValueOnce({ version: '0.3.0', close, downloadAndInstall: install } as never);
    const tree = (active: boolean) => <PageActivity active={active}><AboutSection /></PageActivity>;
    const view = render(tree(true));
    fireEvent.click(screen.getByRole('button', { name: 'Check for Updates' }));
    const confirm = await screen.findByRole('button', { name: 'OK' });
    view.rerender(tree(false));
    view.rerender(tree(true));
    await waitFor(() => expect(close).toHaveBeenCalledOnce());
    // Even a queued click from the closing portal cannot authorize installation.
    fireEvent.click(confirm);
    expect(install).not.toHaveBeenCalled();
  });

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
      close: vi.fn().mockResolvedValue(undefined),
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
      close: vi.fn().mockResolvedValue(undefined),
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
