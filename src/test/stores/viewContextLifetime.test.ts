import { invoke } from '@tauri-apps/api/core';
import { useSdkConfigStore } from '@/stores/sdkConfigStore';
import { useInputStore } from '@/stores/inputStore';
import { retireViewContext } from '@/stores/viewContextLifetime';

describe('retired identity mutations', () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    retireViewContext();
    useSdkConfigStore.getState().reset();
    useInputStore.getState().reset();
  });

  it('does not refresh the new identity when an old config mutation completes for the same Computer', async () => {
    let complete!: () => void;
    vi.mocked(invoke).mockReturnValue(new Promise<void>((resolve) => { complete = resolve; }));
    useSdkConfigStore.setState({ activeInstanceId: 'a' });
    const pending = useSdkConfigStore.getState().removeServer('a', 'server');
    retireViewContext();
    useSdkConfigStore.getState().reset();
    useSdkConfigStore.setState({ activeInstanceId: 'a' });
    complete();
    await pending;
    expect(vi.mocked(invoke)).toHaveBeenCalledTimes(1);
    expect(useSdkConfigStore.getState().pendingMutations).toEqual({});
    expect(useSdkConfigStore.getState().snapshot).toBeNull();
  });

  it('does not publish old input mutation errors or start follow-up reads after identity change', async () => {
    let fail!: (error: Error) => void;
    vi.mocked(invoke).mockReturnValue(new Promise<void>((_resolve, reject) => { fail = reject; }));
    useInputStore.setState({ activeInstanceId: 'a' });
    const pending = useInputStore.getState().upsertEntry('a', 'key', 'value', false);
    retireViewContext();
    useInputStore.getState().reset();
    useInputStore.setState({ activeInstanceId: 'a' });
    fail(new Error('old identity failure'));
    await expect(pending).rejects.toThrow('old identity failure');
    expect(useInputStore.getState().entriesError).toBeNull();
    expect(vi.mocked(invoke)).toHaveBeenCalledTimes(1);
  });
});
