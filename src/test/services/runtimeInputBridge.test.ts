import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  completeRuntimeInputRequest,
  initializeRuntimeInputBridge,
  RuntimeInputCompletionError,
} from '@/services/runtimeInputBridge';
import { useRuntimeInputStore, type RuntimeInputRequest } from '@/stores/runtimeInputStore';

const mockedInvoke = vi.mocked(invoke);
const mockedListen = vi.mocked(listen);

describe('runtimeInputBridge', () => {
  let eventHandler: ((event: { payload: RuntimeInputRequest }) => void) | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    useRuntimeInputStore.getState().reset();
    eventHandler = undefined;
    mockedListen.mockImplementation(async (_event, handler) => {
      eventHandler = handler as (event: { payload: RuntimeInputRequest }) => void;
      return () => undefined;
    });
    mockedInvoke.mockResolvedValue(undefined);
  });

  it('marks the bridge ready before accepting native requests and resets on disposal', async () => {
    const dispose = await initializeRuntimeInputBridge();
    const request: RuntimeInputRequest = {
      requestId: 'request-1',
      instanceId: 'computer-a',
      definition: { type: 'PromptString', id: 'name', default: 'Ada' },
      reason: 'missing',
      secret: false,
    };
    eventHandler?.({ payload: request });

    expect(mockedInvoke).toHaveBeenCalledWith('runtime_input_bridge_ready', {
      leaseId: expect.any(String),
      ready: true,
    });
    expect(useRuntimeInputStore.getState().requests).toEqual([request]);

    await dispose();
    expect(mockedInvoke).toHaveBeenLastCalledWith('runtime_input_bridge_ready', {
      leaseId: expect.any(String),
      ready: false,
    });
    expect(useRuntimeInputStore.getState().requests).toEqual([]);
  });

  it('shares one lease across overlapping owners and shuts down after the last disposal', async () => {
    const [disposeFirst, disposeSecond] = await Promise.all([
      initializeRuntimeInputBridge(),
      initializeRuntimeInputBridge(),
    ]);
    const readyCalls = mockedInvoke.mock.calls.filter(([command, payload]) => (
      command === 'runtime_input_bridge_ready'
      && (payload as { ready?: boolean } | undefined)?.ready === true
    ));

    expect(mockedListen).toHaveBeenCalledTimes(1);
    expect(readyCalls).toHaveLength(1);
    const leaseId = (readyCalls[0][1] as { leaseId: string }).leaseId;

    await disposeFirst();
    expect(mockedInvoke.mock.calls.filter(([command, payload]) => (
      command === 'runtime_input_bridge_ready'
      && (payload as { ready?: boolean } | undefined)?.ready === false
    ))).toHaveLength(0);

    await disposeSecond();
    const shutdownCalls = mockedInvoke.mock.calls.filter(([command, payload]) => (
      command === 'runtime_input_bridge_ready'
      && (payload as { ready?: boolean } | undefined)?.ready === false
    ));
    expect(shutdownCalls).toHaveLength(1);
    expect(shutdownCalls[0][1]).toEqual({ leaseId, ready: false });
  });

  it('forwards confirmation and cancellation without exposing values to the event store', async () => {
    await completeRuntimeInputRequest('request-1', { status: 'confirmed', value: 'Grace' });
    await completeRuntimeInputRequest('request-2', { status: 'cancelled' });

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, 'complete_runtime_input_request', {
      requestId: 'request-1',
      completion: { status: 'confirmed', value: 'Grace' },
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, 'complete_runtime_input_request', {
      requestId: 'request-2',
      completion: { status: 'cancelled' },
    });
  });

  it('cleans up a failed listener and can initialize a fresh lease on retry', async () => {
    mockedListen.mockRejectedValueOnce(new Error('listener unavailable'));

    await expect(initializeRuntimeInputBridge()).rejects.toThrow('listener unavailable');

    const dispose = await initializeRuntimeInputBridge();
    expect(mockedListen).toHaveBeenCalledTimes(2);
    expect(mockedInvoke).toHaveBeenCalledWith('runtime_input_bridge_ready', {
      leaseId: expect.any(String),
      ready: true,
    });
    await dispose();
  });

  it('releases the listener when ready admission fails and recovers with a new lease', async () => {
    const firstUnlisten = vi.fn();
    mockedListen.mockResolvedValueOnce(firstUnlisten);
    mockedInvoke.mockRejectedValueOnce(new Error('ready unavailable'));

    await expect(initializeRuntimeInputBridge()).rejects.toThrow('ready unavailable');
    expect(firstUnlisten).toHaveBeenCalledOnce();

    const dispose = await initializeRuntimeInputBridge();
    await dispose();
  });

  it('finishes local teardown when native shutdown acknowledgement fails', async () => {
    const unlisten = vi.fn();
    mockedListen.mockResolvedValueOnce(unlisten);
    mockedInvoke.mockImplementation(async (command, payload) => {
      if (
        command === 'runtime_input_bridge_ready'
        && (payload as { ready?: boolean } | undefined)?.ready === false
      ) {
        throw new Error('webview is closing');
      }
      return undefined;
    });

    const dispose = await initializeRuntimeInputBridge();
    useRuntimeInputStore.getState().enqueue({
      requestId: 'request-during-teardown',
      instanceId: 'computer-a',
      definition: { type: 'PromptString', id: 'name' },
      reason: 'missing',
      secret: false,
    });

    await expect(dispose()).resolves.toBeUndefined();
    expect(unlisten).toHaveBeenCalledOnce();
    expect(useRuntimeInputStore.getState().requests).toEqual([]);

    const disposeReplacement = await initializeRuntimeInputBridge();
    expect(mockedListen).toHaveBeenCalledTimes(2);
    await expect(disposeReplacement()).resolves.toBeUndefined();
  });

  it('classifies native completion failures as terminal and transport failures as retryable', async () => {
    mockedInvoke
      .mockRejectedValueOnce({
        code: 'inactive',
        terminal: true,
        message: 'request ended',
      })
      .mockRejectedValueOnce(new Error('IPC disconnected'));

    await expect(completeRuntimeInputRequest('request-1', { status: 'cancelled' }))
      .rejects.toMatchObject({
        code: 'inactive',
        terminal: true,
      } satisfies Partial<RuntimeInputCompletionError>);
    await expect(completeRuntimeInputRequest('request-2', { status: 'cancelled' }))
      .rejects.toMatchObject({
        code: 'transport_error',
        terminal: false,
      } satisfies Partial<RuntimeInputCompletionError>);
  });
});
