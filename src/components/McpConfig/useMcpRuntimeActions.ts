import { useEffect, useRef, useState } from 'react';
import {
  formatRuntimeActionError,
  isMissingRuntimeInputError,
  type MissingRuntimeInputError,
} from '@/utils/runtimeActionError';
import type { McpBatchOperationResult } from '@/stores/mcpStore';

export type McpRuntimeAction =
  | { kind: 'start'; bundleId: string; name: string }
  | { kind: 'startAll' };

interface UseMcpRuntimeActionsOptions {
  instanceId: string;
  startServer: (instanceId: string, bundleId: string) => Promise<void>;
  startAll: (instanceId: string) => Promise<McpBatchOperationResult>;
  onBatchResult: (result: McpBatchOperationResult) => void;
  onError: (message: string) => void;
  onSuccess: (action: McpRuntimeAction) => void;
}

export function useMcpRuntimeActions({
  instanceId,
  startServer,
  startAll,
  onBatchResult,
  onError,
  onSuccess,
}: UseMcpRuntimeActionsOptions) {
  const [pending, setPending] = useState<{
    action: McpRuntimeAction;
    error: MissingRuntimeInputError;
  } | null>(null);
  const actionContextRef = useRef({
    instanceId,
    generation: 0,
  });
  if (actionContextRef.current.instanceId !== instanceId) {
    actionContextRef.current = {
      instanceId,
      generation: actionContextRef.current.generation + 1,
    };
  }

  useEffect(() => {
    setPending(null);
    return () => {
      if (actionContextRef.current.instanceId === instanceId) {
        actionContextRef.current = {
          instanceId,
          generation: actionContextRef.current.generation + 1,
        };
      }
    };
  }, [instanceId]);

  const beginAction = () => {
    const context = {
      instanceId,
      generation: actionContextRef.current.generation + 1,
    };
    actionContextRef.current = context;
    return context;
  };
  const isCurrentAction = (context: { instanceId: string; generation: number }) => (
    actionContextRef.current.instanceId === context.instanceId
    && actionContextRef.current.generation === context.generation
  );

  const execute = async (
    action: McpRuntimeAction,
    context: { instanceId: string; generation: number },
  ): Promise<boolean> => {
    try {
      switch (action.kind) {
        case 'start':
          await startServer(context.instanceId, action.bundleId);
          if (!isCurrentAction(context)) return false;
          break;
        case 'startAll': {
          const result = await startAll(context.instanceId);
          if (!isCurrentAction(context)) return false;
          onBatchResult(result);
          const missingInput = result.failures.find((failure) => (
            isMissingRuntimeInputError(failure.error)
          ));
          if (missingInput && isMissingRuntimeInputError(missingInput.error)) {
            setPending({ action, error: missingInput.error });
            return false;
          }
          if (result.failures.length > 0) return false;
          break;
        }
      }
      setPending(null);
      return true;
    } catch (cause) {
      if (!isCurrentAction(context)) return false;
      if (isMissingRuntimeInputError(cause)) {
        setPending({ action, error: cause });
        return false;
      }
      onError(formatRuntimeActionError(cause));
      return false;
    }
  };

  const run = async (action: McpRuntimeAction) => {
    const context = beginAction();
    if (await execute(action, context)) onSuccess(action);
  };

  const retry = async () => {
    if (!pending) return;
    const action = pending.action;
    const context = beginAction();
    if (await execute(action, context)) onSuccess(action);
  };

  return {
    pending,
    run,
    retry,
    cancel: () => {
      actionContextRef.current = {
        instanceId,
        generation: actionContextRef.current.generation + 1,
      };
      setPending(null);
    },
  };
}
