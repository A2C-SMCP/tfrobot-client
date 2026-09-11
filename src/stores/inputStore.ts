import { captureViewContext } from './viewContextLifetime';
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

export interface InputEntry {
  key: string;
  secret: boolean;
  value?: string;
}

export interface InputReferenceIssue {
  inputId: string;
  serverName: string;
  layer: 'project' | 'local';
  fieldPath: string;
}

interface InputState {
  inputs: InputDefinition[];
  entries: InputEntry[];
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
  entriesLoading: boolean;
  entriesLoadedInstanceId: string | null;
  entriesError: string | null;
  entriesRequestId: number;

  fetchInputs: (instanceId: string) => Promise<void>;
  fetchEntries: (instanceId: string) => Promise<void>;
  getEntry: (instanceId: string, key: string) => Promise<InputEntry | null>;
  upsertEntry: (instanceId: string, key: string, value: string | undefined, secret: boolean) => Promise<void>;
  deleteEntry: (instanceId: string, key: string) => Promise<void>;
  fetchValues: (instanceId: string) => Promise<void>;
  refreshValues: (instanceId: string) => Promise<void>;
  fetchReferenceIssues: (instanceId: string) => Promise<void>;
  getInput: (instanceId: string, id: string) => Promise<InputDefinition | null>;
  getRuntimeInput: (instanceId: string, id: string) => Promise<InputDefinition | null>;
  addOrUpdateInput: (instanceId: string, input: InputDefinition) => Promise<void>;
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
  entries: [] as InputEntry[],
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
  entriesLoading: false,
  entriesLoadedInstanceId: null as string | null,
  entriesError: null as string | null,
  entriesRequestId: 0,
};

export const useInputStore = create<InputState>((set, get) => ({
  ...initialState,

  reset: () => set((state) => ({ ...initialState, inputsRequestId: state.inputsRequestId + 1, valuesRequestId: state.valuesRequestId + 1, entriesRequestId: state.entriesRequestId + 1 })),

  fetchEntries: async (instanceId: string) => {
    const requestId = get().entriesRequestId + 1;
    const sameInstance = get().entriesLoadedInstanceId === instanceId;
    set({
      activeInstanceId: instanceId,
      entriesRequestId: requestId,
      entries: sameInstance ? get().entries : [],
      entriesLoading: true,
      entriesLoadedInstanceId: sameInstance ? instanceId : null,
      entriesError: null,
    });
    try {
      const entries = await invoke<InputEntry[]>('list_input_entries', { instanceId });
      if (get().entriesRequestId !== requestId || get().activeInstanceId !== instanceId) return;
      set({ entries, entriesLoading: false, entriesLoadedInstanceId: instanceId });
    } catch (e) {
      if (get().entriesRequestId !== requestId || get().activeInstanceId !== instanceId) return;
      set({ entriesError: String(e), entriesLoading: false });
    }
  },

  getEntry: async (instanceId: string, key: string) => {
    const entries = await invoke<InputEntry[]>('list_input_entries', { instanceId });
    return entries.find((entry) => entry.key === key) ?? null;
  },

  upsertEntry: async (instanceId, key, value, secret) => {
    const currentContext = captureViewContext();
    try {
      await invoke('upsert_input_entry', {
        instanceId,
        key,
        value: value === undefined ? null : value,
        secret,
      });
      if (currentContext() && get().activeInstanceId === instanceId) await get().fetchEntries(instanceId);
    } catch (e) {
      if (currentContext() && get().activeInstanceId === instanceId) set({ entriesError: String(e) });
      throw e;
    }
  },

  deleteEntry: async (instanceId, key) => {
    const currentContext = captureViewContext();
    try {
      await invoke('delete_input_entry', { instanceId, key });
      if (currentContext() && get().activeInstanceId === instanceId) await get().fetchEntries(instanceId);
    } catch (e) {
      if (currentContext() && get().activeInstanceId === instanceId) set({ entriesError: String(e) });
      throw e;
    }
  },

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
    const currentContext = captureViewContext();
    try {
      const referenceIssues = await invoke<InputReferenceIssue[]>('list_input_reference_issues', { instanceId });
      if (currentContext() && get().activeInstanceId === instanceId) set({ referenceIssues });
    } catch (e) {
      if (currentContext()) set({ error: String(e) });
    }
  },

  getInput: async (instanceId: string, id: string) => (
    invoke<InputDefinition | null>('get_input', { instanceId, id })
  ),

  getRuntimeInput: async (instanceId: string, id: string) => (
    invoke<InputDefinition | null>('get_runtime_input', { instanceId, id })
  ),

  addOrUpdateInput: async (instanceId: string, input: InputDefinition) => {
    const currentContext = captureViewContext();
    set({ loading: true, error: null });
    try {
      await invoke('add_or_update_input', { instanceId, input });
      if (currentContext() && get().activeInstanceId === instanceId) await get().fetchInputs(instanceId);
    } catch (e) {
      if (currentContext()) set({ error: String(e), loading: false });
      throw e;
    }
  },

  removeInput: async (instanceId: string, id: string) => {
    const currentContext = captureViewContext();
    set({ loading: true, error: null });
    try {
      await invoke('remove_input', { instanceId, id });
      if (currentContext() && get().activeInstanceId === instanceId) {
        await get().fetchInputs(instanceId);
        if (currentContext()) await get().refreshValues(instanceId);
        if (currentContext()) await get().fetchReferenceIssues(instanceId);
      }
    } catch (e) {
      if (currentContext()) set({ error: String(e), loading: false });
      throw e;
    }
  },

  setValue: async (instanceId: string, id: string, value: unknown) => {
    const currentContext = captureViewContext();
    try {
      await invoke('set_input_value', { instanceId, id, value });
      if (currentContext()) await get().refreshValues(instanceId);
    } catch (e) {
      if (currentContext() && get().activeInstanceId === instanceId) set({ error: String(e) });
      throw e;
    }
  },

  setRuntimeValue: async (instanceId: string, id: string, value: unknown) => {
    const currentContext = captureViewContext();
    try {
      return await invoke<boolean>('set_runtime_input_value', { instanceId, id, value });
    } catch (e) {
      if (currentContext() && get().activeInstanceId === instanceId) set({ error: String(e) });
      throw e;
    }
  },

  removeValue: async (instanceId: string, id: string) => {
    const currentContext = captureViewContext();
    try {
      await invoke('remove_input_value', { instanceId, id });
      if (currentContext()) await get().refreshValues(instanceId);
    } catch (e) {
      if (currentContext() && get().activeInstanceId === instanceId) set({ error: String(e) });
      throw e;
    }
  },

  clearValues: async (instanceId: string) => {
    const currentContext = captureViewContext();
    try {
      await invoke('clear_input_values', { instanceId });
      if (currentContext()) await get().refreshValues(instanceId);
    } catch (e) {
      if (currentContext() && get().activeInstanceId === instanceId) set({ error: String(e) });
      throw e;
    }
  },

  importInputs: async (instanceId: string, path: string) => {
    const currentContext = captureViewContext();
    set({ loading: true, error: null });
    try {
      const count = await invoke<number>('import_inputs', { instanceId, path });
      if (currentContext() && get().activeInstanceId === instanceId) await get().fetchInputs(instanceId);
      return count;
    } catch (e) {
      if (currentContext()) set({ error: String(e), loading: false });
      throw e;
    }
  },
}));
