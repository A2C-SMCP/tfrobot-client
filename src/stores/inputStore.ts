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

export interface InputValueView {
  configured: boolean;
  value?: unknown;
}

interface InputState {
  inputs: InputDefinition[];
  values: Record<string, InputValueView>;
  loading: boolean;
  error: string | null;
  activeInstanceId: string | null;
  inputsRequestId: number;
  valuesRequestId: number;

  fetchInputs: (instanceId: string) => Promise<void>;
  fetchValues: (instanceId: string) => Promise<void>;
  getInput: (instanceId: string, id: string) => Promise<InputDefinition | null>;
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
  values: {} as Record<string, InputValueView>,
  loading: false,
  error: null as string | null,
  activeInstanceId: null as string | null,
  inputsRequestId: 0,
  valuesRequestId: 0,
};

export const useInputStore = create<InputState>((set, get) => ({
  ...initialState,

  reset: () => set(initialState),

  fetchInputs: async (instanceId: string) => {
    const requestId = get().inputsRequestId + 1;
    set({
      activeInstanceId: instanceId,
      inputsRequestId: requestId,
      inputs: [],
      loading: true,
      error: null,
    });
    try {
      const inputs = await invoke<InputDefinition[]>('list_inputs', { instanceId });
      if (get().inputsRequestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({ inputs, loading: false });
    } catch (e) {
      if (get().inputsRequestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({ error: String(e), loading: false });
    }
  },

  fetchValues: async (instanceId: string) => {
    const requestId = get().valuesRequestId + 1;
    set({
      activeInstanceId: instanceId,
      valuesRequestId: requestId,
      values: {},
      error: null,
    });
    try {
      const values = await invoke<Record<string, InputValueView>>('list_input_values', { instanceId });
      if (get().valuesRequestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({ values });
    } catch (e) {
      if (get().valuesRequestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({ error: String(e) });
    }
  },

  getInput: async (instanceId: string, id: string) => (
    invoke<InputDefinition | null>('get_input', { instanceId, id })
  ),

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
