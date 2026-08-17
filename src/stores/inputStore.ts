import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export interface PickOption {
  label: string;
  value: string;
}

export type InputDefinition =
  | { type: 'PromptString'; id: string; label?: string; description?: string; default?: string; password?: boolean }
  | { type: 'PickString'; id: string; label?: string; description?: string; options: PickOption[]; default?: string }
  | { type: 'Command'; id: string; label?: string; command: string; args?: string[] };

export interface InputDefinitionChanges {
  upsert: InputDefinition[];
  removeIfUnused: string[];
}

export function getInputId(input: InputDefinition): string {
  return input.id;
}

export interface InputValueView {
  configured: boolean;
  status: 'configured' | 'using_default' | 'first_option' | 'invalid_selection' | 'missing' | 'runtime_command';
  value?: unknown;
}

export interface InputReferenceIssue {
  inputId: string;
  serverName: string;
  layer: 'project' | 'local';
  fieldPath: string;
}

interface InputState {
  inputs: InputDefinition[];
  values: Record<string, InputValueView>;
  referenceIssues: InputReferenceIssue[];
  loading: boolean;
  error: string | null;
  valuesLoading: boolean;
  valuesLoadedInstanceId: string | null;
  valuesError: string | null;
  activeInstanceId: string | null;
  inputsRequestId: number;
  valuesRequestId: number;

  fetchInputs: (instanceId: string) => Promise<void>;
  fetchValues: (instanceId: string) => Promise<void>;
  refreshValues: (instanceId: string) => Promise<void>;
  fetchReferenceIssues: (instanceId: string) => Promise<void>;
  getInput: (instanceId: string, id: string) => Promise<InputDefinition | null>;
  addOrUpdateInput: (instanceId: string, input: InputDefinition) => Promise<void>;
  saveInput: (
    instanceId: string,
    input: InputDefinition,
    value?: string,
    keepExistingValue?: boolean,
  ) => Promise<void>;
  removeInput: (instanceId: string, id: string) => Promise<void>;
  setValue: (instanceId: string, id: string, value: unknown) => Promise<void>;
  setRuntimeValue: (instanceId: string, id: string, value: unknown) => Promise<boolean>;
  removeValue: (instanceId: string, id: string) => Promise<void>;
  clearValues: (instanceId: string) => Promise<void>;
  importInputs: (instanceId: string, path: string) => Promise<number>;
  reset: () => void;
}

const initialState = {
  inputs: [] as InputDefinition[],
  values: {} as Record<string, InputValueView>,
  referenceIssues: [] as InputReferenceIssue[],
  loading: false,
  error: null as string | null,
  valuesLoading: false,
  valuesLoadedInstanceId: null as string | null,
  valuesError: null as string | null,
  activeInstanceId: null as string | null,
  inputsRequestId: 0,
  valuesRequestId: 0,
};

export const useInputStore = create<InputState>((set, get) => ({
  ...initialState,

  reset: () => set(initialState),

  fetchInputs: async (instanceId: string) => {
    const requestId = get().inputsRequestId + 1;
    const changingInstance = get().activeInstanceId !== instanceId;
    set({
      activeInstanceId: instanceId,
      inputsRequestId: requestId,
      inputs: [],
      loading: true,
      error: null,
      ...(changingInstance ? {
        values: {},
        valuesLoading: false,
        valuesLoadedInstanceId: null,
        valuesError: null,
      } : {}),
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
      valuesLoading: true,
      valuesLoadedInstanceId: null,
      valuesError: null,
    });
    try {
      const values = await invoke<Record<string, InputValueView>>('list_input_values', { instanceId });
      if (get().valuesRequestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({ values, valuesLoading: false, valuesLoadedInstanceId: instanceId });
    } catch (e) {
      if (get().valuesRequestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({ valuesError: String(e), valuesLoading: false, valuesLoadedInstanceId: null });
    }
  },

  refreshValues: async (instanceId: string) => {
    if (get().activeInstanceId !== instanceId) return;
    const requestId = get().valuesRequestId + 1;
    set({ valuesRequestId: requestId, valuesLoading: true, valuesError: null });
    try {
      const values = await invoke<Record<string, InputValueView>>('list_input_values', { instanceId });
      if (get().valuesRequestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({ values, valuesLoading: false, valuesLoadedInstanceId: instanceId });
    } catch (e) {
      if (get().valuesRequestId !== requestId || get().activeInstanceId !== instanceId) {
        return;
      }
      set({ valuesError: String(e), valuesLoading: false, valuesLoadedInstanceId: null });
    }
  },

  fetchReferenceIssues: async (instanceId: string) => {
    try {
      const referenceIssues = await invoke<InputReferenceIssue[]>('list_input_reference_issues', { instanceId });
      if (get().activeInstanceId === instanceId) set({ referenceIssues });
    } catch (e) {
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
      if (get().activeInstanceId === instanceId) await get().fetchInputs(instanceId);
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  saveInput: async (instanceId, input, value, keepExistingValue = false) => {
    set({ loading: true, error: null });
    try {
      await invoke('save_input', {
        instanceId,
        input,
        value: value ?? null,
        keepExistingValue,
      });
      if (get().activeInstanceId === instanceId) {
        await get().fetchInputs(instanceId);
        await get().refreshValues(instanceId);
        await get().fetchReferenceIssues(instanceId);
      }
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  removeInput: async (instanceId: string, id: string) => {
    set({ loading: true, error: null });
    try {
      await invoke('remove_input', { instanceId, id });
      if (get().activeInstanceId === instanceId) {
        await get().fetchInputs(instanceId);
        await get().refreshValues(instanceId);
        await get().fetchReferenceIssues(instanceId);
      }
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  setValue: async (instanceId: string, id: string, value: unknown) => {
    try {
      await invoke('set_input_value', { instanceId, id, value });
      await get().refreshValues(instanceId);
    } catch (e) {
      if (get().activeInstanceId === instanceId) set({ error: String(e) });
      throw e;
    }
  },

  setRuntimeValue: async (instanceId: string, id: string, value: unknown) => {
    try {
      return await invoke<boolean>('set_runtime_input_value', { instanceId, id, value });
    } catch (e) {
      if (get().activeInstanceId === instanceId) set({ error: String(e) });
      throw e;
    }
  },

  removeValue: async (instanceId: string, id: string) => {
    try {
      await invoke('remove_input_value', { instanceId, id });
      await get().refreshValues(instanceId);
    } catch (e) {
      if (get().activeInstanceId === instanceId) set({ error: String(e) });
      throw e;
    }
  },

  clearValues: async (instanceId: string) => {
    try {
      await invoke('clear_input_values', { instanceId });
      await get().refreshValues(instanceId);
    } catch (e) {
      if (get().activeInstanceId === instanceId) set({ error: String(e) });
      throw e;
    }
  },

  importInputs: async (instanceId: string, path: string) => {
    set({ loading: true, error: null });
    try {
      const count = await invoke<number>('import_inputs', { instanceId, path });
      if (get().activeInstanceId === instanceId) await get().fetchInputs(instanceId);
      return count;
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },
}));
