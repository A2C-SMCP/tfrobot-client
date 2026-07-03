import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export interface SkillRef {
  name: string;
  source: string;
  uri?: string | null;
  path: string;
  description: string;
  license?: string | null;
  compatibility?: string | null;
  allowed_tools?: string[] | null;
  version?: string | null;
}

export interface SkillResource {
  name: string;
  relPath: string;
  mimeType: string;
  totalSize: number;
  sha256: string;
  isEntry: boolean;
  isText: boolean;
  body?: string | null;
}

export interface MarketplaceCapabilities {
  computerLifecycleApiAvailable: boolean;
  supportedOperations: string[];
  requiredSdkApis: string[];
  reason: string;
}

export interface MarketplaceSummary {
  name: string;
  gitUrl?: string | null;
  status: string;
  message?: string | null;
}

export interface PluginSummary {
  marketplace: string;
  plugin: string;
  pluginId?: string | null;
  version?: string | null;
  enabled: boolean;
  status: string;
  bundledMcpServers: string[];
  bundledSkills: string[];
  message?: string | null;
}

export interface MarketplaceGovernance {
  capabilities: MarketplaceCapabilities;
  marketplaces: MarketplaceSummary[];
  plugins: PluginSummary[];
}

export interface AddMarketplaceRequest {
  name: string;
  gitUrl: string;
}

export interface PluginLifecycleRequest {
  marketplace: string;
  plugin: string;
}

interface InstanceSkillRecord {
  skills: SkillRef[];
  selectedSkillName: string | null;
  selectedSkill: SkillResource | null;
  governance: MarketplaceGovernance | null;
  loadingSkills: boolean;
  loadingSkill: boolean;
  loadingMarketplace: boolean;
  error: string | null;
  skillError: string | null;
  marketplaceError: string | null;
  skillsRequestId: number;
  skillRequestId: number;
  marketplaceRequestId: number;
}

interface SkillState extends InstanceSkillRecord {
  capabilities: MarketplaceCapabilities | null;
  marketplaces: MarketplaceSummary[];
  plugins: PluginSummary[];
  activeInstanceId: string | null;
  recordsByInstanceId: Record<string, InstanceSkillRecord>;

  fetchSkills: (instanceId: string) => Promise<void>;
  refreshSkills: (instanceId: string) => Promise<void>;
  selectSkill: (instanceId: string, name: string) => Promise<void>;
  openLocalSkillsRoot: (instanceId: string) => Promise<void>;
  fetchMarketplaceCapabilities: (instanceId: string) => Promise<void>;
  fetchMarketplaceGovernance: (instanceId: string) => Promise<void>;
  addMarketplace: (instanceId: string, request: AddMarketplaceRequest) => Promise<void>;
  updateMarketplace: (instanceId: string, request: AddMarketplaceRequest) => Promise<void>;
  refreshMarketplace: (instanceId: string, marketplace: string) => Promise<void>;
  removeMarketplace: (instanceId: string, marketplace: string) => Promise<void>;
  installPlugin: (instanceId: string, request: PluginLifecycleRequest) => Promise<void>;
  enablePlugin: (instanceId: string, request: PluginLifecycleRequest) => Promise<void>;
  disablePlugin: (instanceId: string, request: PluginLifecycleRequest) => Promise<void>;
  uninstallPlugin: (instanceId: string, request: PluginLifecycleRequest) => Promise<void>;
  reset: () => void;
}

const emptyRecord: InstanceSkillRecord = {
  skills: [],
  selectedSkillName: null,
  selectedSkill: null,
  governance: null,
  loadingSkills: false,
  loadingSkill: false,
  loadingMarketplace: false,
  error: null,
  skillError: null,
  marketplaceError: null,
  skillsRequestId: 0,
  skillRequestId: 0,
  marketplaceRequestId: 0,
};

const initialState = {
  ...emptyRecord,
  capabilities: null as MarketplaceCapabilities | null,
  marketplaces: [] as MarketplaceSummary[],
  plugins: [] as PluginSummary[],
  activeInstanceId: null as string | null,
  recordsByInstanceId: {} as Record<string, InstanceSkillRecord>,
};

function cloneEmptyRecord(): InstanceSkillRecord {
  return {
    ...emptyRecord,
    skills: [],
  };
}

function viewFromRecord(
  instanceId: string,
  record: InstanceSkillRecord,
): Pick<
  SkillState,
  | 'activeInstanceId'
  | 'skills'
  | 'selectedSkillName'
  | 'selectedSkill'
  | 'governance'
  | 'capabilities'
  | 'marketplaces'
  | 'plugins'
  | 'loadingSkills'
  | 'loadingSkill'
  | 'loadingMarketplace'
  | 'error'
  | 'skillError'
  | 'marketplaceError'
  | 'skillsRequestId'
  | 'skillRequestId'
  | 'marketplaceRequestId'
> {
  return {
    activeInstanceId: instanceId,
    skills: record.skills,
    selectedSkillName: record.selectedSkillName,
    selectedSkill: record.selectedSkill,
    governance: record.governance,
    capabilities: record.governance?.capabilities ?? null,
    marketplaces: record.governance?.marketplaces ?? [],
    plugins: record.governance?.plugins ?? [],
    loadingSkills: record.loadingSkills,
    loadingSkill: record.loadingSkill,
    loadingMarketplace: record.loadingMarketplace,
    error: record.error,
    skillError: record.skillError,
    marketplaceError: record.marketplaceError,
    skillsRequestId: record.skillsRequestId,
    skillRequestId: record.skillRequestId,
    marketplaceRequestId: record.marketplaceRequestId,
  };
}

function setInstanceRecord(
  set: (partial: Partial<SkillState> | ((state: SkillState) => Partial<SkillState>)) => void,
  instanceId: string,
  updater: (record: InstanceSkillRecord) => InstanceSkillRecord,
) {
  set((state) => {
    const current = state.recordsByInstanceId[instanceId] ?? cloneEmptyRecord();
    const record = updater(current);
    return {
      recordsByInstanceId: {
        ...state.recordsByInstanceId,
        [instanceId]: record,
      },
      ...viewFromRecord(instanceId, record),
    };
  });
}

function isCurrentRequest(
  state: SkillState,
  instanceId: string,
  requestId: number,
  kind: 'skillsRequestId' | 'skillRequestId' | 'marketplaceRequestId',
) {
  return state.activeInstanceId === instanceId
    && state.recordsByInstanceId[instanceId]?.[kind] === requestId;
}

async function invokeMarketplace<T>(
  command: string,
  instanceId: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  return await invoke<T>(command, { instanceId, ...args });
}

export function formatInvokeError(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'string') return error;
  if (error && typeof error === 'object') {
    const message = (error as { message?: unknown }).message;
    if (typeof message === 'string' && message.trim()) return message;
    try {
      return JSON.stringify(error);
    } catch {
      return Object.prototype.toString.call(error);
    }
  }
  return String(error);
}

export const useSkillStore = create<SkillState>((set, get) => ({
  ...initialState,

  fetchSkills: async (instanceId) => {
    const requestId = (get().recordsByInstanceId[instanceId]?.skillsRequestId ?? 0) + 1;
    setInstanceRecord(set, instanceId, (record) => ({
      ...record,
      skillsRequestId: requestId,
      skills: [],
      selectedSkillName: null,
      selectedSkill: null,
      loadingSkills: true,
      error: null,
      skillError: null,
    }));
    try {
      const skills = await invoke<SkillRef[]>('list_skills', { instanceId });
      if (!isCurrentRequest(get(), instanceId, requestId, 'skillsRequestId')) return;
      setInstanceRecord(set, instanceId, (record) => ({
        ...record,
        skills,
        loadingSkills: false,
      }));
    } catch (e) {
      if (!isCurrentRequest(get(), instanceId, requestId, 'skillsRequestId')) return;
      setInstanceRecord(set, instanceId, (record) => ({
        ...record,
        error: formatInvokeError(e),
        loadingSkills: false,
      }));
    }
  },

  refreshSkills: async (instanceId) => {
    const requestId = (get().recordsByInstanceId[instanceId]?.skillsRequestId ?? 0) + 1;
    setInstanceRecord(set, instanceId, (record) => ({
      ...record,
      skillsRequestId: requestId,
      loadingSkills: true,
      error: null,
    }));
    try {
      await invoke('refresh_skills', { instanceId });
      if (!isCurrentRequest(get(), instanceId, requestId, 'skillsRequestId')) return;
      await get().fetchSkills(instanceId);
    } catch (e) {
      if (!isCurrentRequest(get(), instanceId, requestId, 'skillsRequestId')) return;
      setInstanceRecord(set, instanceId, (record) => ({
        ...record,
        error: formatInvokeError(e),
        loadingSkills: false,
      }));
      throw e;
    }
  },

  selectSkill: async (instanceId, name) => {
    const requestId = (get().recordsByInstanceId[instanceId]?.skillRequestId ?? 0) + 1;
    setInstanceRecord(set, instanceId, (record) => ({
      ...record,
      skillRequestId: requestId,
      selectedSkillName: name,
      selectedSkill: null,
      loadingSkill: true,
      skillError: null,
    }));
    try {
      const selectedSkill = await invoke<SkillResource>('get_skill', {
        instanceId,
        name,
        relPath: null,
      });
      if (!isCurrentRequest(get(), instanceId, requestId, 'skillRequestId')) return;
      setInstanceRecord(set, instanceId, (record) => ({
        ...record,
        selectedSkill,
        loadingSkill: false,
      }));
    } catch (e) {
      if (!isCurrentRequest(get(), instanceId, requestId, 'skillRequestId')) return;
      setInstanceRecord(set, instanceId, (record) => ({
        ...record,
        skillError: formatInvokeError(e),
        loadingSkill: false,
      }));
    }
  },

  openLocalSkillsRoot: async (instanceId) => {
    await invoke('open_local_skills_root', { instanceId });
  },

  fetchMarketplaceCapabilities: async (instanceId) => {
    await get().fetchMarketplaceGovernance(instanceId);
  },

  fetchMarketplaceGovernance: async (instanceId) => {
    const requestId = (get().recordsByInstanceId[instanceId]?.marketplaceRequestId ?? 0) + 1;
    setInstanceRecord(set, instanceId, (record) => ({
      ...record,
      marketplaceRequestId: requestId,
      governance: null,
      loadingMarketplace: true,
      marketplaceError: null,
    }));
    try {
      const governance = await invoke<MarketplaceGovernance>('get_marketplace_governance', { instanceId });
      if (!isCurrentRequest(get(), instanceId, requestId, 'marketplaceRequestId')) return;
      setInstanceRecord(set, instanceId, (record) => ({
        ...record,
        governance,
        loadingMarketplace: false,
      }));
    } catch (e) {
      if (!isCurrentRequest(get(), instanceId, requestId, 'marketplaceRequestId')) return;
      setInstanceRecord(set, instanceId, (record) => ({
        ...record,
        marketplaceError: formatInvokeError(e),
        loadingMarketplace: false,
      }));
    }
  },

  addMarketplace: async (instanceId, request) => {
    await runMarketplaceLifecycle(set, get, instanceId, () => invokeMarketplace('add_marketplace', instanceId, { request }));
  },

  updateMarketplace: async (instanceId, request) => {
    await runMarketplaceLifecycle(set, get, instanceId, () => invokeMarketplace('update_marketplace', instanceId, { request }));
  },

  refreshMarketplace: async (instanceId, marketplace) => {
    await runMarketplaceLifecycle(set, get, instanceId, () => invokeMarketplace('refresh_marketplace', instanceId, { marketplace }));
  },

  removeMarketplace: async (instanceId, marketplace) => {
    await runMarketplaceLifecycle(set, get, instanceId, () => invokeMarketplace('remove_marketplace', instanceId, { marketplace }));
  },

  installPlugin: async (instanceId, request) => {
    await runMarketplaceLifecycle(set, get, instanceId, () => invokeMarketplace('install_plugin', instanceId, { request }));
  },

  enablePlugin: async (instanceId, request) => {
    await runMarketplaceLifecycle(set, get, instanceId, () => invokeMarketplace('enable_plugin', instanceId, { request }));
  },

  disablePlugin: async (instanceId, request) => {
    await runMarketplaceLifecycle(set, get, instanceId, () => invokeMarketplace('disable_plugin', instanceId, { request }));
  },

  uninstallPlugin: async (instanceId, request) => {
    await runMarketplaceLifecycle(set, get, instanceId, () => invokeMarketplace('uninstall_plugin', instanceId, { request }));
  },

  reset: () => set(initialState),
}));

async function runMarketplaceLifecycle(
  set: (partial: Partial<SkillState> | ((state: SkillState) => Partial<SkillState>)) => void,
  get: () => SkillState,
  instanceId: string,
  action: () => Promise<unknown>,
) {
  const requestId = (get().recordsByInstanceId[instanceId]?.marketplaceRequestId ?? 0) + 1;
  setInstanceRecord(set, instanceId, (record) => ({
    ...record,
    marketplaceRequestId: requestId,
    loadingMarketplace: true,
    marketplaceError: null,
  }));
  try {
    await action();
    if (!isCurrentRequest(get(), instanceId, requestId, 'marketplaceRequestId')) return;
    await get().fetchMarketplaceGovernance(instanceId);
    if (get().activeInstanceId === instanceId) {
      await get().fetchSkills(instanceId);
    }
  } catch (e) {
    if (!isCurrentRequest(get(), instanceId, requestId, 'marketplaceRequestId')) return;
    setInstanceRecord(set, instanceId, (record) => ({
      ...record,
      marketplaceError: formatInvokeError(e),
      loadingMarketplace: false,
    }));
    throw e;
  }
}
