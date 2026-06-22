import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export interface PickOption {
  label: string;
  value: string;
}

export type InputDefinition =
  | { type: 'PromptString'; id: string; label: string; description?: string; default?: string; password?: boolean }
  | { type: 'PickString'; id: string; label: string; description?: string; options: PickOption[]; default?: string }
  | { type: 'Command'; id: string; label: string; command: string; args?: string[] };

export function getInputId(input: InputDefinition): string {
  return input.id;
}

interface InputState {
  inputs: InputDefinition[];
  values: Record<string, unknown>;
  loading: boolean;
  error: string | null;

  fetchInputs: (instanceId: string) => Promise<void>;
  fetchValues: (instanceId: string) => Promise<void>;
  addOrUpdateInput: (instanceId: string, input: InputDefinition) => Promise<void>;
  removeInput: (instanceId: string, id: string) => Promise<void>;
  setValue: (instanceId: string, id: string, value: unknown) => Promise<void>;
  removeValue: (instanceId: string, id: string) => Promise<void>;
  clearValues: (instanceId: string) => Promise<void>;
  importInputs: (instanceId: string, path: string) => Promise<number>;
  reset: () => void;
}

const initialState = {
  inputs: [] as InputDefinition[],
  values: {} as Record<string, unknown>,
  loading: false,
  error: null as string | null,
};

export const useInputStore = create<InputState>((set, get) => ({
  ...initialState,

  reset: () => set(initialState),

  fetchInputs: async (instanceId: string) => {
    set({ loading: true, error: null });
    try {
      const inputs = await invoke<InputDefinition[]>('list_inputs', { instanceId });
      set({ inputs, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  fetchValues: async (instanceId: string) => {
    try {
      const values = await invoke<Record<string, unknown>>('list_input_values', { instanceId });
      set({ values });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  addOrUpdateInput: async (instanceId: string, input: InputDefinition) => {
    set({ loading: true, error: null });
    try {
      await invoke('add_or_update_input', { instanceId, input });
      await get().fetchInputs(instanceId);
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  removeInput: async (instanceId: string, id: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('remove_input', { instanceId, id });
      await get().fetchInputs(instanceId);
      await get().fetchValues(instanceId);
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  setValue: async (instanceId: string, id: string, value: unknown) => {
    try {
      await invoke('set_input_value', { instanceId, id, value });
      await get().fetchValues(instanceId);
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  removeValue: async (instanceId: string, id: string) => {
    try {
      await invoke('remove_input_value', { instanceId, id });
      await get().fetchValues(instanceId);
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  clearValues: async (instanceId: string) => {
    try {
      await invoke('clear_input_values', { instanceId });
      set({ values: {} });
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  importInputs: async (instanceId: string, path: string) => {
    set({ loading: true, error: null });
    try {
      const count = await invoke<number>('import_inputs', { instanceId, path });
      await get().fetchInputs(instanceId);
      return count;
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },
}));
