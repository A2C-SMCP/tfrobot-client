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

export type PortableSectionStatus =
  | 'compatible'
  | 'migratable'
  | 'incompatible'
  | 'conflict'
  | 'missing';

export interface PortableMarketplaceDeclaration {
  name: string;
  source: unknown;
  commitSha?: string;
}

export interface PortableSectionPreview {
  group: PortableConfigGroup;
  status: PortableSectionStatus;
  message?: string;
}

export interface PortablePackagePreview {
  originalName: string;
  finalName: string;
  nameConflict: boolean;
  formatVersion: number;
  versionCompatible: boolean;
  versionMessage?: string;
  sections: PortableSectionPreview[];
  marketplaces: PortableMarketplaceDeclaration[];
  installedPlugins: string[];
}

export interface PortableImportResult {
  id: string;
  name: string;
}

interface PortableConfigState {
  exporting: boolean;
  previewing: boolean;
  committing: boolean;
  error: string | null;
  exportPackage: (instanceId: string, path: string, groups: PortableConfigGroup[]) => Promise<void>;
  previewImport: (path: string) => Promise<PortablePackagePreview>;
  commitImport: (path: string, finalName: string) => Promise<PortableImportResult>;
  reset: () => void;
}

const initialState = {
  exporting: false,
  previewing: false,
  committing: false,
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

  previewImport: async (path) => {
    set({ previewing: true, error: null });
    try {
      return await invoke<PortablePackagePreview>('preview_computer_package_import', { path });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    } finally {
      set({ previewing: false });
    }
  },

  commitImport: async (path, finalName) => {
    set({ committing: true, error: null });
    try {
      return await invoke<PortableImportResult>('commit_computer_package_import', {
        path,
        finalName,
      });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    } finally {
      set({ committing: false });
    }
  },

  reset: () => set(initialState),
}));
