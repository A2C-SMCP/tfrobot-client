import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export type ComputerStatus = 'running' | 'stopped' | 'error';
export type ComputerConnectionStatus = 'connected' | 'disconnected';

export interface RobotBindingMetadata {
  employee_id: number;
  robot_id?: string;
  robot_account_id?: number;
  namespace?: string;
  robot_name?: string;
}

export interface ConnectionStateSummary {
  url: string;
  office_id: string;
  computer_name: string;
  connected_at: string;
  profile_name: string;
}

export interface ComputerInstanceStatus {
  id: string;
  name: string;
  is_default: boolean;
  running: boolean;
  connected: boolean;
  mcp_server_count: number;
  robot_binding?: RobotBindingMetadata | null;
  connection?: ConnectionStateSummary | null;
}

export interface ComputerInstance {
  id: string;
  name: string;
  isDefault: boolean;
  status: ComputerStatus;
  connectionStatus: ComputerConnectionStatus;
  connectionProfile?: string;
  robotName?: string;
  mcpServerCount: number;
}

interface ComputerState {
  instances: ComputerInstance[];
  loading: boolean;
  error: string | null;
  selectedInstanceId: string | null;
  fetchInstances: () => Promise<void>;
  selectInstance: (id: string) => void;
  reset: () => void;
}

const initialState = {
  instances: [] as ComputerInstance[],
  loading: false,
  error: null as string | null,
  selectedInstanceId: null as string | null,
};

function toComputerInstance(status: ComputerInstanceStatus): ComputerInstance {
  return {
    id: status.id,
    name: status.name,
    isDefault: status.is_default,
    status: status.running ? 'running' : 'stopped',
    connectionStatus: status.connected ? 'connected' : 'disconnected',
    connectionProfile: status.connection?.profile_name,
    robotName: status.robot_binding?.robot_name,
    mcpServerCount: status.mcp_server_count,
  };
}

export const useComputerStore = create<ComputerState>((set) => ({
  ...initialState,

  fetchInstances: async () => {
    set({ loading: true, error: null });
    try {
      const statuses = await invoke<ComputerInstanceStatus[]>('list_computer_instances');
      const instances = statuses.map(toComputerInstance);
      set((state) => ({
        instances,
        loading: false,
        selectedInstanceId: instances.some((instance) => instance.id === state.selectedInstanceId)
          ? state.selectedInstanceId
          : instances[0]?.id ?? null,
      }));
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  selectInstance: (id: string) => set({ selectedInstanceId: id }),

  reset: () => set(initialState),
}));
