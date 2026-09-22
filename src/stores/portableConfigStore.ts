import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export type PortableConfigGroup =
  | 'basic_profile'
  | 'mcp_and_inputs'
  | 'non_sensitive_input_values'
  | 'skills_and_plugins';

export const ALL_PORTABLE_CONFIG_GROUPS: PortableConfigGroup[] = [
  'basic_profile',
  'mcp_and_inputs',
  'non_sensitive_input_values',
  'skills_and_plugins',
];

export interface PortablePackageInspection {
  originalName: string;
  description?: string;
  formatVersion: number;
  groups: PortableConfigGroup[];
}

export type PortableInspection = PortablePackageInspection;

interface PortableConfigState {
  exporting: boolean;
  inspecting: boolean;
  error: string | null;
  exportPackage: (instanceId: string, path: string, groups: PortableConfigGroup[]) => Promise<void>;
  inspectPackage: (path: string) => Promise<PortablePackageInspection>;
  reset: () => void;
}

const initialState = {
  exporting: false,
  inspecting: false,
  error: null as string | null,
};

export const usePortableConfigStore = create<PortableConfigState>((set) => ({
  ...initialState,

  exportPackage: async (instanceId, path, groups) => {
    set({ exporting: true, error: null });
    try {
      await invoke('export_computer_package', {
        instanceId,
        path,
        groups: groups.length > 0 ? groups : null,
      });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    } finally {
      set({ exporting: false });
    }
  },

  inspectPackage: async (path) => {
    set({ inspecting: true, error: null });
    try {
      return await invoke<PortablePackageInspection>('inspect_computer_package', { path });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    } finally {
      set({ inspecting: false });
    }
  },

  reset: () => set(initialState),
}));
