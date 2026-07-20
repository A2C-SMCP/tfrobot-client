import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import type { McpServerConfig } from './mcpStore';
import { formatRuntimeActionError } from '@/utils/runtimeActionError';

export interface SdkConfigServer {
  name: string;
  origin: string;
  writable: boolean;
  trustedOrigin: boolean;
  bundled: boolean;
  config: McpServerConfig;
}

export interface SdkConfigSnapshot {
  version: number;
  revision: string;
  mcp: { servers: SdkConfigServer[] };
  provenance: Record<string, string>;
}

export interface SdkConfigValidationError {
  scope: string;
  field: string;
  reason: string;
  source_path?: string;
}

export interface SdkConfigValidation {
  valid: boolean;
  errors: SdkConfigValidationError[];
}

interface SdkConfigStateResponse {
  snapshot: SdkConfigSnapshot;
  validation: SdkConfigValidation;
}

export interface ImportResult {
  servers_imported: number;
  inputs_imported: number;
  servers_skipped: string[];
}

interface SdkConfigState {
  snapshot: SdkConfigSnapshot | null;
  validation: SdkConfigValidation | null;
  loading: boolean;
  validating: boolean;
  error: string | null;
  activeInstanceId: string | null;
  requestId: number;
  validationRequestId: number;
  pendingMutations: Record<string, number>;
  fetchConfig: (instanceId: string) => Promise<void>;
  validateConfig: (instanceId: string) => Promise<void>;
  upsertServer: (instanceId: string, config: McpServerConfig) => Promise<void>;
  removeServer: (instanceId: string, name: string) => Promise<void>;
  importConfig: (instanceId: string, path: string) => Promise<ImportResult>;
  exportConfig: (instanceId: string, path: string, serverNames?: string[]) => Promise<void>;
  reset: () => void;
}

const initialState = {
  snapshot: null as SdkConfigSnapshot | null,
  validation: null as SdkConfigValidation | null,
  loading: false,
  validating: false,
  error: null as string | null,
  activeInstanceId: null as string | null,
  requestId: 0,
  validationRequestId: 0,
  pendingMutations: {} as Record<string, number>,
};

type SdkConfigSet = (
  partial: Partial<SdkConfigState> | ((state: SdkConfigState) => Partial<SdkConfigState>)
) => void;
type SdkConfigGet = () => SdkConfigState;

function beginMutation(set: SdkConfigSet, get: SdkConfigGet, instanceId: string) {
  const pendingMutations = {
    ...get().pendingMutations,
    [instanceId]: (get().pendingMutations[instanceId] ?? 0) + 1,
  };
  const ownsUi = get().activeInstanceId === null || get().activeInstanceId === instanceId;
  set({
    pendingMutations,
    ...(ownsUi ? { activeInstanceId: instanceId, loading: true, error: null } : {}),
  });
}

function finishMutation(set: SdkConfigSet, get: SdkConfigGet, instanceId: string) {
  const pendingMutations = { ...get().pendingMutations };
  const remaining = Math.max((pendingMutations[instanceId] ?? 1) - 1, 0);
  if (remaining === 0) delete pendingMutations[instanceId];
  else pendingMutations[instanceId] = remaining;
  set({
    pendingMutations,
    ...(get().activeInstanceId === instanceId ? { loading: remaining > 0 } : {}),
  });
}

function reportMutationError(
  set: SdkConfigSet,
  get: SdkConfigGet,
  instanceId: string,
  cause: unknown,
) {
  if (get().activeInstanceId === instanceId) {
    set({ error: formatRuntimeActionError(cause) });
  }
}

export const useSdkConfigStore = create<SdkConfigState>((set, get) => ({
  ...initialState,

  fetchConfig: async (instanceId: string) => {
    const requestId = get().requestId + 1;
    const validationRequestId = get().validationRequestId + 1;
    const sameInstance = get().activeInstanceId === instanceId;
    set({
      activeInstanceId: instanceId,
      requestId,
      validationRequestId,
      snapshot: sameInstance ? get().snapshot : null,
      validation: sameInstance ? get().validation : null,
      loading: true,
      validating: true,
      error: null,
    });
    try {
      const { snapshot, validation } = await invoke<SdkConfigStateResponse>(
        'get_computer_config_state',
        { instanceId },
      );
      const current = get();
      if (current.requestId !== requestId || current.activeInstanceId !== instanceId) return;
      const validationIsCurrent = current.validationRequestId === validationRequestId;
      set({
        loading: (current.pendingMutations[instanceId] ?? 0) > 0,
        ...(validationIsCurrent ? { snapshot, validation, validating: false } : {}),
      });
    } catch (cause) {
      const current = get();
      if (current.requestId !== requestId || current.activeInstanceId !== instanceId) return;
      const validationIsCurrent = current.validationRequestId === validationRequestId;
      set({
        loading: (current.pendingMutations[instanceId] ?? 0) > 0,
        ...(validationIsCurrent
          ? { error: formatRuntimeActionError(cause), validating: false }
          : {}),
      });
    }
  },

  validateConfig: async (instanceId: string) => {
    const validationRequestId = get().validationRequestId + 1;
    set({
      activeInstanceId: instanceId,
      validationRequestId,
      validating: true,
      error: null,
    });
    try {
      const { snapshot, validation } = await invoke<SdkConfigStateResponse>(
        'get_computer_config_state',
        { instanceId },
      );
      const current = get();
      if (
        current.validationRequestId !== validationRequestId
        || current.activeInstanceId !== instanceId
      ) return;
      set({ snapshot, validation, validating: false });
    } catch (cause) {
      const current = get();
      if (
        current.validationRequestId !== validationRequestId
        || current.activeInstanceId !== instanceId
      ) return;
      set({ error: formatRuntimeActionError(cause), validating: false });
    }
  },

  upsertServer: async (instanceId: string, config: McpServerConfig) => {
    beginMutation(set, get, instanceId);
    try {
      await invoke('upsert_computer_mcp_config', { instanceId, config });
      if (get().activeInstanceId === instanceId) await get().fetchConfig(instanceId);
    } catch (cause) {
      reportMutationError(set, get, instanceId, cause);
      throw cause;
    } finally {
      finishMutation(set, get, instanceId);
    }
  },

  removeServer: async (instanceId: string, name: string) => {
    beginMutation(set, get, instanceId);
    try {
      await invoke('remove_computer_mcp_config', { instanceId, name });
      if (get().activeInstanceId === instanceId) await get().fetchConfig(instanceId);
    } catch (cause) {
      reportMutationError(set, get, instanceId, cause);
      throw cause;
    } finally {
      finishMutation(set, get, instanceId);
    }
  },

  importConfig: async (instanceId: string, path: string) => {
    beginMutation(set, get, instanceId);
    try {
      const result = await invoke<ImportResult>('import_config', {
        path,
        instanceId,
        format: null,
      });
      if (get().activeInstanceId === instanceId) await get().fetchConfig(instanceId);
      return result;
    } catch (cause) {
      reportMutationError(set, get, instanceId, cause);
      throw cause;
    } finally {
      finishMutation(set, get, instanceId);
    }
  },

  exportConfig: async (instanceId: string, path: string, serverNames?: string[]) => {
    beginMutation(set, get, instanceId);
    try {
      await invoke('export_config', { path, instanceId, serverNames: serverNames ?? null });
    } catch (cause) {
      reportMutationError(set, get, instanceId, cause);
      throw cause;
    } finally {
      finishMutation(set, get, instanceId);
    }
  },

  reset: () => set(initialState),
}));
