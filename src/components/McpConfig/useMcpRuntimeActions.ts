import { useEffect, useState } from 'react';
import {
  formatRuntimeActionError,
  isMissingRuntimeInputError,
  type MissingRuntimeInputError,
} from '@/utils/runtimeActionError';

export type McpRuntimeAction =
  | { kind: 'start'; bundleId: string; name: string }
  | { kind: 'startAll' };

interface UseMcpRuntimeActionsOptions {
  instanceId: string;
  startServer: (instanceId: string, bundleId: string) => Promise<void>;
  startAll: (instanceId: string) => Promise<void>;
  onError: (message: string) => void;
  onSuccess: (action: McpRuntimeAction) => void;
}

export function useMcpRuntimeActions({
  instanceId,
  startServer,
  startAll,
  onError,
  onSuccess,
}: UseMcpRuntimeActionsOptions) {
  const [pending, setPending] = useState<{
    action: McpRuntimeAction;
    error: MissingRuntimeInputError;
  } | null>(null);

  useEffect(() => setPending(null), [instanceId]);

  const execute = async (action: McpRuntimeAction): Promise<boolean> => {
    try {
      switch (action.kind) {
        case 'start':
          await startServer(instanceId, action.bundleId);
          break;
        case 'startAll':
          await startAll(instanceId);
          break;
      }
      setPending(null);
      return true;
    } catch (cause) {
      if (isMissingRuntimeInputError(cause)) {
        setPending({ action, error: cause });
        return false;
      }
      onError(formatRuntimeActionError(cause));
      return false;
    }
  };

  const run = async (action: McpRuntimeAction) => {
    if (await execute(action)) onSuccess(action);
  };

  const retry = async () => {
    if (pending && await execute(pending.action)) onSuccess(pending.action);
  };

  return {
    pending,
    run,
    retry,
    cancel: () => setPending(null),
  };
}
