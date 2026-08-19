import { create } from 'zustand';
import type { InputDefinition } from './inputStore';

export interface RuntimeInputRequest {
  requestId: string;
  instanceId: string;
  definition: InputDefinition;
  reason: 'missing' | 'invalid_selection';
  secret: boolean;
}

interface RuntimeInputState {
  requests: RuntimeInputRequest[];
  completionFailed: boolean;
  enqueue: (request: RuntimeInputRequest) => void;
  remove: (requestId: string) => void;
  reportCompletionFailure: () => void;
  clearCompletionFailure: () => void;
  reset: () => void;
}

export const useRuntimeInputStore = create<RuntimeInputState>((set) => ({
  requests: [],
  completionFailed: false,
  enqueue: (request) => set((state) => (
    state.requests.some((candidate) => candidate.requestId === request.requestId)
      ? state
      : { requests: [...state.requests, request] }
  )),
  remove: (requestId) => set((state) => ({
    requests: state.requests.filter((request) => request.requestId !== requestId),
  })),
  reportCompletionFailure: () => set({ completionFailed: true }),
  clearCompletionFailure: () => set({ completionFailed: false }),
  reset: () => set({ requests: [], completionFailed: false }),
}));
