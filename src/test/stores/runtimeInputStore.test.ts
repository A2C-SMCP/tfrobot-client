import { beforeEach, describe, expect, it } from 'vitest';
import { useRuntimeInputStore, type RuntimeInputRequest } from '@/stores/runtimeInputStore';

const request = (requestId: string): RuntimeInputRequest => ({
  requestId,
  instanceId: 'computer-a',
  definition: { type: 'PromptString', id: requestId },
  reason: 'missing',
  secret: false,
});

describe('runtimeInputStore', () => {
  beforeEach(() => useRuntimeInputStore.getState().reset());

  it('keeps requests in arrival order and ignores duplicate events', () => {
    const store = useRuntimeInputStore.getState();
    store.enqueue(request('first'));
    store.enqueue(request('second'));
    store.enqueue(request('first'));

    expect(useRuntimeInputStore.getState().requests.map((item) => item.requestId))
      .toEqual(['first', 'second']);
  });

  it('removes only the completed request', () => {
    const store = useRuntimeInputStore.getState();
    store.enqueue(request('first'));
    store.enqueue(request('second'));
    store.remove('first');

    expect(useRuntimeInputStore.getState().requests).toEqual([request('second')]);
  });
});
