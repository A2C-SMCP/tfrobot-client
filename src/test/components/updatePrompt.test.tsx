import { invoke } from '@tauri-apps/api/core';
import { presentUpdatePrompt } from '@/components/ApplicationUpdate/updatePrompt';

describe('update prompt controller', () => {
  beforeEach(() => vi.clearAllMocks());

  function setup(downloadAndInstall: () => Promise<void>) {
    let config!: Parameters<Parameters<typeof presentUpdatePrompt>[0]['modal']['confirm']>[0];
    const close = vi.fn().mockResolvedValue(undefined);
    const modal = {
      confirm: vi.fn((next) => {
        config = next;
        return { destroy: vi.fn() };
      }),
    };
    const update = {
      version: '0.3.0',
      close,
      downloadAndInstall,
    } as never;
    const onInstallError = vi.fn();
    const controller = presentUpdatePrompt({
      update,
      modal,
      title: 'Update',
      content: 'Notes',
      cancelText: 'Later',
      trigger: 'manual',
      isActive: () => true,
      onInstallError,
    });
    return { config, close, controller, onInstallError };
  }

  it('persists a deferred version and releases the updater handle', async () => {
    const { config, close } = setup(vi.fn().mockResolvedValue(undefined));

    config.onCancel?.({} as never);
    await Promise.resolve();

    expect(invoke).toHaveBeenCalledWith('set_deferred_update_version', { version: '0.3.0' });
    expect(close).toHaveBeenCalledOnce();
  });

  it('records installation failures and releases the updater handle', async () => {
    const error = new Error('signature rejected');
    const { config, close, onInstallError } = setup(vi.fn().mockRejectedValue(error));

    await config.onOk?.({} as never);

    expect(invoke).toHaveBeenCalledWith(
      'record_application_update_activity',
      expect.objectContaining({ activity: 'install_failed', targetVersion: '0.3.0' }),
    );
    expect(onInstallError).toHaveBeenCalledWith(error);
    expect(close).toHaveBeenCalledOnce();
  });
});
