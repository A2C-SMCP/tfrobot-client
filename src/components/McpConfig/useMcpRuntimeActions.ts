import { useEffect, useState } from 'react';
import type { McpServerConfig } from '@/stores/mcpStore';
import {
  formatRuntimeActionError,
  isMissingRuntimeInputError,
  type MissingRuntimeInputError,
} from '@/utils/runtimeActionError';

export type McpRuntimeAction =
  | { kind: 'add'; config: McpServerConfig }
  | { kind: 'update'; config: McpServerConfig }
  | { kind: 'start'; name: string }
  | { kind: 'startAll' };

interface UseMcpRuntimeActionsOptions {
  instanceId: string;
  addServer: (instanceId: string, config: McpServerConfig) => Promise<void>;
  updateServer: (instanceId: string, config: McpServerConfig) => Promise<void>;
  startServer: (instanceId: string, name: string) => Promise<void>;
  startAll: (instanceId: string) => Promise<void>;
  onError: (message: string) => void;
  onSuccess: (action: McpRuntimeAction) => void;
}

export function useMcpRuntimeActions({
  instanceId,
  addServer,
  updateServer,
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
        case 'add':
          await addServer(instanceId, action.config);
          break;
        case 'update':
          await updateServer(instanceId, action.config);
          break;
        case 'start':
          await startServer(instanceId, action.name);
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
