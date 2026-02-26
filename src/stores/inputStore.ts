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

  fetchInputs: () => Promise<void>;
  fetchValues: () => Promise<void>;
  addOrUpdateInput: (input: InputDefinition) => Promise<void>;
  removeInput: (id: string) => Promise<void>;
  setValue: (id: string, value: unknown) => Promise<void>;
  removeValue: (id: string) => Promise<void>;
  clearValues: () => Promise<void>;
  importInputs: (path: string) => Promise<number>;
}

export const useInputStore = create<InputState>((set, get) => ({
  inputs: [],
  values: {},
  loading: false,
  error: null,

  fetchInputs: async () => {
    set({ loading: true, error: null });
    try {
      const inputs = await invoke<InputDefinition[]>('list_inputs');
      set({ inputs, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  fetchValues: async () => {
    try {
      const values = await invoke<Record<string, unknown>>('list_input_values');
      set({ values });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  addOrUpdateInput: async (input: InputDefinition) => {
    set({ loading: true, error: null });
    try {
      await invoke('add_or_update_input', { input });
      await get().fetchInputs();
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  removeInput: async (id: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('remove_input', { id });
      await get().fetchInputs();
      await get().fetchValues();
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  setValue: async (id: string, value: unknown) => {
    try {
      await invoke('set_input_value', { id, value });
      await get().fetchValues();
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  removeValue: async (id: string) => {
    try {
      await invoke('remove_input_value', { id });
      await get().fetchValues();
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  clearValues: async () => {
    try {
      await invoke('clear_input_values');
      set({ values: {} });
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  importInputs: async (path: string) => {
    set({ loading: true, error: null });
    try {
      const count = await invoke<number>('import_inputs', { path });
      await get().fetchInputs();
      return count;
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },
}));
