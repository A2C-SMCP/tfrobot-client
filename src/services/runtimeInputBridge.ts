import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useRuntimeInputStore, type RuntimeInputRequest } from '@/stores/runtimeInputStore';

const RUNTIME_INPUT_REQUEST_EVENT = 'runtime-input:request';

export type RuntimeInputCompletion = {
  status: 'confirmed';
  value: string;
} | {
  status: 'cancelled';
};

interface NativeRuntimeInputCompletionError {
  code: string;
  terminal: boolean;
  message: string;
}

export class RuntimeInputCompletionError extends Error {
  constructor(
    message: string,
    readonly terminal: boolean,
    readonly code = 'transport_error',
  ) {
    super(message);
    this.name = 'RuntimeInputCompletionError';
  }
}

function isNativeCompletionError(error: unknown): error is NativeRuntimeInputCompletionError {
  if (typeof error !== 'object' || error === null) return false;
  const candidate = error as Partial<NativeRuntimeInputCompletionError>;
  return typeof candidate.code === 'string'
    && typeof candidate.terminal === 'boolean'
    && typeof candidate.message === 'string';
}

interface BridgeSession {
  leaseId: string;
  references: number;
  unlisten: UnlistenFn | null;
  start: Promise<void>;
  shutdown: Promise<void> | null;
}

let activeSession: BridgeSession | null = null;
let leaseSequence = 0;

async function startSession(session: BridgeSession): Promise<void> {
  try {
    session.unlisten = await listen<RuntimeInputRequest>(RUNTIME_INPUT_REQUEST_EVENT, (event) => {
      useRuntimeInputStore.getState().enqueue(event.payload);
    });
    await invoke('runtime_input_bridge_ready', {
      leaseId: session.leaseId,
      ready: true,
    });
  } catch (error) {
    session.unlisten?.();
    session.unlisten = null;
    throw error;
  }
}

function createSession(): BridgeSession {
  const session = {
    leaseId: `runtime-input-bridge-${++leaseSequence}`,
    references: 0,
    unlisten: null,
    start: Promise.resolve(),
    shutdown: null,
  } satisfies BridgeSession;
  session.start = startSession(session);
  return session;
}

export async function initializeRuntimeInputBridge(): Promise<() => Promise<void>> {
  if (activeSession?.shutdown) {
    await activeSession.shutdown;
    return initializeRuntimeInputBridge();
  }
  const session = activeSession ?? createSession();
  activeSession = session;
  session.references += 1;
  try {
    await session.start;
  } catch (error) {
    session.references -= 1;
    if (session.references === 0 && activeSession === session) activeSession = null;
    throw error;
  }

  let disposed = false;
  return async () => {
    if (disposed) return;
    disposed = true;
    session.references -= 1;
    if (session.references > 0) return;
    if (!session.shutdown) {
      session.shutdown = (async () => {
        try {
          await invoke('runtime_input_bridge_ready', {
            leaseId: session.leaseId,
            ready: false,
          });
        } catch {
          // Webview teardown can invalidate IPC before native acknowledgement arrives. This is
          // best-effort shutdown: local ownership must still be released, and a later session's
          // new lease replaces any stale native bridge authority.
        } finally {
          session.unlisten?.();
          session.unlisten = null;
          useRuntimeInputStore.getState().reset();
          if (activeSession === session) activeSession = null;
        }
      })();
    }
    await session.shutdown;
  };
}

export async function completeRuntimeInputRequest(
  requestId: string,
  completion: RuntimeInputCompletion,
): Promise<void> {
  try {
    await invoke('complete_runtime_input_request', { requestId, completion });
  } catch (error) {
    if (isNativeCompletionError(error)) {
      throw new RuntimeInputCompletionError(error.message, error.terminal, error.code);
    }
    // An unstructured invoke failure may have happened before the native command was admitted,
    // so callers may safely keep the request visible and let the user retry.
    throw new RuntimeInputCompletionError(String(error), false);
  }
}
