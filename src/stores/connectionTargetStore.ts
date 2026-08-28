import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import { debug, error as logError } from '@/utils/logger';
import { useComputerStore } from './computerStore';

export interface ManualSmcpTarget {
  id: string;
  name: string;
  url: string;
  namespace: string;
  office_id: string;
  headers: Record<string, string>;
}

export type ManualSmcpApiKeyAction =
  | { kind: 'unchanged' }
  | { kind: 'set'; value: string }
  | { kind: 'clear' };

interface ConnectionTargetState {
  manualTargets: ManualSmcpTarget[];
  loading: boolean;
  error: string | null;
  fetchManualTargets: () => Promise<void>;
  saveManualTarget: (
    target: ManualSmcpTarget,
    apiKeyAction?: ManualSmcpApiKeyAction,
  ) => Promise<ManualSmcpTarget>;
  deleteManualTarget: (targetId: string) => Promise<void>;
  connectTarget: (instanceId: string, targetId: string) => Promise<void>;
  reset: () => void;
}

const initialState = {
  manualTargets: [] as ManualSmcpTarget[],
  loading: false,
  error: null as string | null,
};

export const useConnectionTargetStore = create<ConnectionTargetState>((set, get) => ({
  ...initialState,

  fetchManualTargets: async () => {
    set({ loading: true, error: null });
    try {
      const manualTargets = await invoke<ManualSmcpTarget[]>('list_manual_smcp_targets');
      set({ manualTargets, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  saveManualTarget: async (target, apiKeyAction = { kind: 'unchanged' }) => {
    set({ loading: true, error: null });
    try {
      const saved = await invoke<ManualSmcpTarget>('save_manual_smcp_target', {
        target,
        apiKeyAction,
      });
      await get().fetchManualTargets();
      return saved;
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  deleteManualTarget: async (targetId) => {
    set({ loading: true, error: null });
    try {
      await invoke('delete_manual_smcp_target', { targetId });
      await get().fetchManualTargets();
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  connectTarget: async (instanceId, targetId) => {
    const startedAt = Date.now();
    debug(`connection.requested layer=frontend operation=connect source_type=manual_smcp instance_id=${instanceId} target_id=${targetId}`);
    set({ loading: true, error: null });
    try {
      await invoke('connect_connection_target', { instanceId, targetId });
      debug(`connection.request_completed layer=frontend operation=connect source_type=manual_smcp instance_id=${instanceId} target_id=${targetId} elapsed_ms=${Date.now() - startedAt} outcome=succeeded`);
    } catch (e) {
      logError(`connection.request_failed layer=frontend operation=connect source_type=manual_smcp instance_id=${instanceId} target_id=${targetId} elapsed_ms=${Date.now() - startedAt}`);
      set({ error: String(e), loading: false });
      throw e;
    }
    try {
      // The runtime event owns connection state. This one-shot reconciliation is only for the
      // persisted connection policy changed by the command.
      await useComputerStore.getState().reconcileConnectionMetadata(instanceId);
      set({ loading: false });
    } catch (e) {
      logError(`connection.metadata_reconciliation_failed layer=frontend source_type=manual_smcp instance_id=${instanceId} target_id=${targetId} elapsed_ms=${Date.now() - startedAt} connection_outcome=succeeded`);
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  reset: () => set(initialState),
}));
