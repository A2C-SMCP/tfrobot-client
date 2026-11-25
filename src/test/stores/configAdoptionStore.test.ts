import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { useConfigAdoptionStore, type ConfigAdoptionStatus } from '@/stores/configAdoptionStore';
const call = vi.mocked(invoke);
const status = (state: ConfigAdoptionStatus['state']): ConfigAdoptionStatus => ({ incarnation: 1, generation: 1, state, changes: [], unknown_resources: [] });
const settle = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };

describe('configuration adoption observations', () => {
  beforeEach(() => { useConfigAdoptionStore.getState().reset(); vi.clearAllMocks(); });
  it('replaces a result invalidated during flight without displaying it as current', async () => {
    let release!: (value: ConfigAdoptionStatus) => void;
    call.mockImplementationOnce(() => new Promise((resolve) => { release = resolve; }));
    call.mockResolvedValueOnce(status('pending'));
    const unwatch = useConfigAdoptionStore.getState().watch('a');
    useConfigAdoptionStore.getState().invalidate('a');
    release(status('current'));
    await settle();
    expect(call).toHaveBeenCalledTimes(2);
    expect(useConfigAdoptionStore.getState().statuses.a?.state).toBe('pending');
    unwatch();
  });
  it('keeps instances isolated and rejects responses from a reset session', async () => {
    let release!: (value: ConfigAdoptionStatus) => void;
    call.mockImplementationOnce(() => new Promise((resolve) => { release = resolve; }));
    useConfigAdoptionStore.getState().watch('a');
    useConfigAdoptionStore.getState().reset();
    call.mockResolvedValueOnce(status('waiting_start'));
    const unwatch = useConfigAdoptionStore.getState().watch('b');
    release(status('current'));
    await settle();
    expect(useConfigAdoptionStore.getState().statuses.a).toBeUndefined();
    expect(useConfigAdoptionStore.getState().statuses.b?.state).toBe('waiting_start');
    unwatch();
  });
  it('marks failed observation unknown and retries only when explicitly invalidated', async () => {
    call.mockRejectedValueOnce(new Error('unavailable'));
    const unwatch = useConfigAdoptionStore.getState().watch('a');
    await settle();
    expect(useConfigAdoptionStore.getState().errors.a).toBe(true);
    expect(call).toHaveBeenCalledTimes(1);
    call.mockResolvedValueOnce(status('current'));
    useConfigAdoptionStore.getState().invalidate('a');
    await settle();
    expect(useConfigAdoptionStore.getState().errors.a).toBe(false);
    unwatch();
  });
});
