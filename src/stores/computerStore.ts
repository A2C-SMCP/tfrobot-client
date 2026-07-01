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

export type ComputerConnectionTargetType = 'manager_robot' | 'manual_smcp';

export interface ComputerConnectionTarget {
  type: ComputerConnectionTargetType;
  id: string;
  robotAccountId?: number;
}

export interface ComputerConnectionPolicy {
  target?: ComputerConnectionTarget | null;
  auto_connect: boolean;
}

export interface ComputerInstanceStatus {
  id: string;
  name: string;
  description?: string;
  local_skills_root?: string | null;
  effective_skill_home?: string;
  running: boolean;
  connected: boolean;
  mcp_server_count: number;
  robot_binding?: RobotBindingMetadata | null;
  connection_policy?: ComputerConnectionPolicy;
  connection?: ConnectionStateSummary | null;
}

export interface ComputerInstance {
  id: string;
  name: string;
  description?: string;
  status: ComputerStatus;
  connectionStatus: ComputerConnectionStatus;
  connectionProfile?: string;
  robotName?: string;
  robotBinding?: RobotBindingMetadata | null;
  localSkillsRoot?: string | null;
  effectiveSkillHome?: string;
  connectionPolicy: ComputerConnectionPolicy;
  mcpServerCount: number;
}

export interface ComputerFormValues {
  name: string;
  description?: string;
}

export type DuplicateSkillHomeMode = 'empty' | 'copy';

export interface DuplicateComputerValues extends ComputerFormValues {
  sourceId: string;
  copyRobotBinding: boolean;
  connectionTargetId?: string;
  skillHomeMode: DuplicateSkillHomeMode;
}

interface ComputerState {
  instances: ComputerInstance[];
  loading: boolean;
  error: string | null;
  selectedInstanceId: string | null;
  fetchInstances: () => Promise<void>;
  createInstance: (values: ComputerFormValues) => Promise<ComputerInstance>;
  updateInstance: (id: string, values: ComputerFormValues) => Promise<ComputerInstance>;
  duplicateInstance: (values: DuplicateComputerValues) => Promise<ComputerInstance>;
  deleteInstance: (id: string) => Promise<void>;
  startInstance: (id: string) => Promise<ComputerInstance>;
  stopInstance: (id: string) => Promise<ComputerInstance>;
  updateConnectionPolicy: (
    id: string,
    policy: ComputerConnectionPolicy,
  ) => Promise<ComputerInstance>;
  updateSkillHome: (id: string, localSkillsRoot?: string | null) => Promise<ComputerInstance>;
  connectSelectedTarget: (id: string) => Promise<void>;
  disconnectConnection: (id: string) => Promise<void>;
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
    description: status.description ?? undefined,
    status: status.running ? 'running' : 'stopped',
    connectionStatus: status.connected ? 'connected' : 'disconnected',
    connectionProfile: status.connection?.profile_name,
    robotName: status.robot_binding?.robot_name,
    robotBinding: status.robot_binding,
    localSkillsRoot: status.local_skills_root ?? null,
    effectiveSkillHome: status.effective_skill_home,
    connectionPolicy: status.connection_policy ?? { target: null, auto_connect: false },
    mcpServerCount: status.mcp_server_count,
  };
}

function normalizeFormValues(values: ComputerFormValues): ComputerFormValues {
  const name = values.name.trim();
  const description = values.description?.trim();
  return {
    name,
    description: description || undefined,
  };
}

function upsertInstance(instances: ComputerInstance[], instance: ComputerInstance): ComputerInstance[] {
  const index = instances.findIndex((item) => item.id === instance.id);
  if (index === -1) return [...instances, instance];
  return instances.map((item) => (item.id === instance.id ? instance : item));
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

  createInstance: async (values) => {
    set({ loading: true, error: null });
    try {
      const created = toComputerInstance(await invoke<ComputerInstanceStatus>('create_computer_instance', {
        request: normalizeFormValues(values),
      }));
      set((state) => ({
        instances: upsertInstance(state.instances, created),
        selectedInstanceId: created.id,
        loading: false,
      }));
      return created;
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  updateInstance: async (id, values) => {
    set({ loading: true, error: null });
    try {
      const updated = toComputerInstance(await invoke<ComputerInstanceStatus>('rename_computer_instance', {
        request: { id, ...normalizeFormValues(values) },
      }));
      set((state) => ({
        instances: upsertInstance(state.instances, updated),
        selectedInstanceId: state.selectedInstanceId,
        loading: false,
      }));
      return updated;
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  duplicateInstance: async (values) => {
    set({ loading: true, error: null });
    try {
      const request = {
        sourceId: values.sourceId,
        ...normalizeFormValues(values),
        copyRobotBinding: values.copyRobotBinding,
        connectionTargetId: values.connectionTargetId || undefined,
        skillHomeMode: values.skillHomeMode,
      };
      const duplicated = toComputerInstance(await invoke<ComputerInstanceStatus>('duplicate_computer_instance', {
        request,
      }));
      set((state) => ({
        instances: upsertInstance(state.instances, duplicated),
        selectedInstanceId: duplicated.id,
        loading: false,
      }));
      return duplicated;
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  deleteInstance: async (id) => {
    set({ loading: true, error: null });
    try {
      await invoke('delete_computer_instance', { id });
      set((state) => {
        const instances = state.instances.filter((instance) => instance.id !== id);
        return {
          instances,
          selectedInstanceId: state.selectedInstanceId === id
            ? instances[0]?.id ?? null
            : state.selectedInstanceId,
          loading: false,
        };
      });
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  startInstance: async (id) => {
    set({ loading: true, error: null });
    try {
      const started = toComputerInstance(await invoke<ComputerInstanceStatus>('start_computer_instance', { id }));
      set((state) => ({
        instances: upsertInstance(state.instances, started),
        selectedInstanceId: state.selectedInstanceId,
        loading: false,
      }));
      return started;
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  stopInstance: async (id) => {
    set({ loading: true, error: null });
    try {
      const stopped = toComputerInstance(await invoke<ComputerInstanceStatus>('stop_computer_instance', { id }));
      set((state) => ({
        instances: upsertInstance(state.instances, stopped),
        selectedInstanceId: state.selectedInstanceId,
        loading: false,
      }));
      return stopped;
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  updateConnectionPolicy: async (id, policy) => {
    set({ loading: true, error: null });
    try {
      const updated = toComputerInstance(await invoke<ComputerInstanceStatus>('update_computer_connection_policy', {
        request: {
          id,
          target: policy.target ?? null,
          autoConnect: policy.auto_connect,
        },
      }));
      set((state) => ({
        instances: upsertInstance(state.instances, updated),
        selectedInstanceId: state.selectedInstanceId,
        loading: false,
      }));
      return updated;
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  updateSkillHome: async (id, localSkillsRoot) => {
    set({ loading: true, error: null });
    try {
      const updated = toComputerInstance(await invoke<ComputerInstanceStatus>('update_computer_skill_home', {
        request: {
          id,
          localSkillsRoot: localSkillsRoot || null,
        },
      }));
      set((state) => ({
        instances: upsertInstance(state.instances, updated),
        selectedInstanceId: state.selectedInstanceId,
        loading: false,
      }));
      return updated;
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  connectSelectedTarget: async (id) => {
    set({ loading: true, error: null });
    try {
      await invoke('connect_computer_connection_target', { id });
      const statuses = await invoke<ComputerInstanceStatus[]>('list_computer_instances');
      const instances = statuses.map(toComputerInstance);
      set((state) => ({
        instances,
        selectedInstanceId: instances.some((instance) => instance.id === state.selectedInstanceId)
          ? state.selectedInstanceId
          : instances[0]?.id ?? null,
        loading: false,
      }));
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  disconnectConnection: async (id) => {
    set({ loading: true, error: null });
    try {
      await invoke('disconnect_computer_connection_target', { id });
      const statuses = await invoke<ComputerInstanceStatus[]>('list_computer_instances');
      const instances = statuses.map(toComputerInstance);
      set((state) => ({
        instances,
        selectedInstanceId: instances.some((instance) => instance.id === state.selectedInstanceId)
          ? state.selectedInstanceId
          : instances[0]?.id ?? null,
        loading: false,
      }));
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  selectInstance: (id: string) => set({ selectedInstanceId: id }),

  reset: () => set(initialState),
}));
