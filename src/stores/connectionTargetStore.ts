import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import { useComputerStore } from './computerStore';
import { useConnectionStore } from './connectionStore';

export interface ManualSmcpTarget {
  id: string;
  name: string;
  url: string;
  namespace: string;
  office_id: string;
  computer_name: string;
  headers: Record<string, string>;
}

interface ConnectionTargetState {
  manualTargets: ManualSmcpTarget[];
  loading: boolean;
  error: string | null;
  fetchManualTargets: () => Promise<void>;
  saveManualTarget: (target: ManualSmcpTarget, apiKey?: string) => Promise<ManualSmcpTarget>;
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

  saveManualTarget: async (target, apiKey) => {
    set({ loading: true, error: null });
    try {
      const saved = await invoke<ManualSmcpTarget>('save_manual_smcp_target', {
        target,
        apiKey: apiKey || null,
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
    set({ loading: true, error: null });
    try {
      await invoke('connect_connection_target', { instanceId, targetId });
      await useConnectionStore.getState().fetchStatus(instanceId);
      await useComputerStore.getState().fetchInstances();
      set({ loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  reset: () => set(initialState),
}));
