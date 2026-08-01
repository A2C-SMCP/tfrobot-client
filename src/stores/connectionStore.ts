import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import { info } from '@/utils/logger';
import {
  getClientConnectionAuthority,
  type ClientConnectionActionCapabilities,
  type ClientConnectionAuthority,
  type ClientConnectionOperation,
  type ClientConnectionOperationError,
  type ClientConnectionOperationTarget,
  type ClientConnectionStatus,
} from './connectionAuthority';
import type { ComputerRuntimeSnapshot } from './runtimeSnapshot';

export interface ConnectionStatusInfo {
  status?: ClientConnectionStatus;
  connected: boolean;
  operation?: ClientConnectionOperation;
  operation_target?: ClientConnectionOperationTarget;
  last_error?: ClientConnectionOperationError;
  actions?: ClientConnectionActionCapabilities;
  url?: string;
  office_id?: string;
  computer_name?: string;
  connected_at?: string;
  profile_name?: string;
  source_type?: 'manual_smcp' | 'manager_robot';
  target_id?: string;
  target_name?: string;
  employee_id?: number;
  connection_state?: ClientConnectionAuthority;
}

interface ConnectionState {
  statuses: Record<string, ConnectionStatusInfo>;
  loading: boolean;
  error: string | null;

  getStatus: (instanceId: string) => ConnectionStatusInfo;
  applyRuntimeSnapshot: (instanceId: string, runtime: ComputerRuntimeSnapshot) => void;
  forgetStatus: (instanceId: string) => void;
  disconnect: (instanceId: string) => Promise<void>;
  reset: () => void;
}

const initialState = {
  statuses: {} as Record<string, ConnectionStatusInfo>,
  loading: false,
  error: null as string | null,
};

export const useConnectionStore = create<ConnectionState>((set, get) => ({
  ...initialState,

  getStatus: (instanceId: string) => get().statuses[instanceId] ?? {
    status: 'disconnected',
    connected: false,
  },

  applyRuntimeSnapshot: (instanceId, runtime) => {
    const authority = getClientConnectionAuthority(instanceId, runtime.incarnation);
    const context = authority?.context;
    const status: ConnectionStatusInfo = context ? {
      status: authority?.status ?? 'disconnected',
      connected: authority?.status === 'connected',
      operation: authority?.operation ?? undefined,
      operation_target: authority?.operation_target ?? undefined,
      last_error: authority?.last_error ?? undefined,
      actions: authority?.actions,
      url: context.url,
      office_id: context.office_id,
      computer_name: context.computer_name,
      connected_at: context.connected_at,
      profile_name: context.profile_name,
      source_type: context.source_type === 'manual_smcp' || context.source_type === 'manager_robot'
        ? context.source_type
        : undefined,
      target_id: context.target_id ?? undefined,
      target_name: context.target_name ?? undefined,
      employee_id: context.employee_id ?? undefined,
    } : {
      status: authority?.status ?? 'disconnected',
      connected: authority?.status === 'connected',
      operation: authority?.operation ?? undefined,
      operation_target: authority?.operation_target ?? undefined,
      last_error: authority?.last_error ?? undefined,
      actions: authority?.actions,
    };
    set((state) => ({
      statuses: {
        ...state.statuses,
        [instanceId]: status,
      },
    }));
  },

  forgetStatus: (instanceId) => set((state) => {
    if (!(instanceId in state.statuses)) return {};
    const statuses = { ...state.statuses };
    delete statuses[instanceId];
    return { statuses };
  }),

  reset: () => set(initialState),

  disconnect: async (instanceId: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('disconnect_smcp', { instanceId });
      info('SMCP disconnected');
      set({ loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },
}));
