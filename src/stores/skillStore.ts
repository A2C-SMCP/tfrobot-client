import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import { isRuntimeInputCancelledError } from '@/utils/runtimeActionError';

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
  source: MarketplaceSourceSummary;
  status: string;
  message?: string | null;
}

export type MarketplaceSourceSummary =
  | { type: 'remoteGit'; displayGitUrl?: string | null }
  | { type: 'localGit'; path: string };

export interface PluginSummary {
  marketplace: string;
  plugin: string;
  pluginId?: string | null;
  version?: string | null;
  installed: boolean;
  enabled: boolean;
  status: string;
  bundledMcpServers: string[];
  bundledSkills: string[];
  declared: DeclaredPluginCapabilities | null;
  message?: string | null;
}

export interface DeclaredPluginCapabilities {
  version?: string | null;
  description?: string | null;
  mcpServers: string[];
  skills: string[];
}

export interface MarketplaceGovernance {
  capabilities: MarketplaceCapabilities;
  marketplaces: MarketplaceSummary[];
  plugins: PluginSummary[];
}

export interface AddMarketplaceRequest {
  name: string;
  source: MarketplaceSource;
}

export type MarketplaceSource =
  | { type: 'remoteGit'; gitUrl: string }
  | { type: 'localGit'; path: string };

export interface PluginLifecycleRequest {
  marketplace: string;
  plugin: string;
}

export type MarketplaceOperationKind =
  | 'add'
  | 'update'
  | 'refresh'
  | 'remove'
  | 'install'
  | 'enable'
  | 'disable'
  | 'uninstall';

export interface MarketplaceOperation {
  id: number;
  kind: MarketplaceOperationKind;
  target: string;
}

interface InstanceSkillRecord {
  skills: SkillRef[];
  selectedSkillName: string | null;
  selectedSkill: SkillResource | null;
  governance: MarketplaceGovernance | null;
  loadingSkills: boolean;
  loadingSkill: boolean;
  loadingMarketplace: boolean;
  marketplaceOperation: MarketplaceOperation | null;
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
  openConfiguredLocalSkillsRoot: (instanceId: string) => Promise<void>;
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
  marketplaceOperation: null,
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
  | 'marketplaceOperation'
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
    marketplaceOperation: record.marketplaceOperation,
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
  projectActiveView = true,
) {
  set((state) => {
    const current = state.recordsByInstanceId[instanceId] ?? cloneEmptyRecord();
    const record = updater(current);
    return {
      recordsByInstanceId: {
        ...state.recordsByInstanceId,
        [instanceId]: record,
      },
      ...(projectActiveView ? viewFromRecord(instanceId, record) : {}),
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

let navigationRequestSequence = 0;

export const useSkillStore = create<SkillState>((set, get) => ({
  ...initialState,

  fetchSkills: async (instanceId) => {
    const requestId = ++navigationRequestSequence;
    setInstanceRecord(set, instanceId, (record) => ({
      ...record,
      skillsRequestId: requestId,
      skills: record.skills,
      selectedSkillName: record.selectedSkillName,
      selectedSkill: record.selectedSkill,
      loadingSkills: true,
      error: null,
      skillError: null,
    }));
    try {
      const skills = await invoke<SkillRef[]>('list_skills', { instanceId });
      if (!isCurrentRequest(get(), instanceId, requestId, 'skillsRequestId')) return;
      setInstanceRecord(set, instanceId, (record) => {
        const selectedExists = skills.some((skill) => skill.name === record.selectedSkillName);
        return {
          ...record, skills, loadingSkills: false,
          ...(!selectedExists ? {
            selectedSkillName: null, selectedSkill: null, loadingSkill: false, skillError: null,
            skillRequestId: ++navigationRequestSequence,
          } : {}),
        };
      });
      const selectedName = get().recordsByInstanceId[instanceId]?.selectedSkillName;
      if (selectedName && isCurrentRequest(get(), instanceId, requestId, 'skillsRequestId')) {
        await get().selectSkill(instanceId, selectedName);
      }
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
    const requestId = ++navigationRequestSequence;
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
    const requestId = ++navigationRequestSequence;
    setInstanceRecord(set, instanceId, (record) => ({
      ...record,
      skillRequestId: requestId,
      selectedSkillName: name,
      selectedSkill: record.selectedSkillName === name ? record.selectedSkill : null,
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

  openConfiguredLocalSkillsRoot: async (instanceId) => {
    await invoke('open_configured_local_skills_root', { instanceId });
  },

  fetchMarketplaceCapabilities: async (instanceId) => {
    await get().fetchMarketplaceGovernance(instanceId);
  },

  fetchMarketplaceGovernance: async (instanceId) => {
    const requestId = ++navigationRequestSequence;
    setInstanceRecord(set, instanceId, (record) => ({
      ...record,
      marketplaceRequestId: requestId,
      governance: record.governance,
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
    await runMarketplaceLifecycle(
      set,
      get,
      instanceId,
      { kind: 'add', target: request.name },
      () => invokeMarketplace('add_marketplace', instanceId, { request }),
    );
  },

  updateMarketplace: async (instanceId, request) => {
    await runMarketplaceLifecycle(
      set,
      get,
      instanceId,
      { kind: 'update', target: request.name },
      () => invokeMarketplace('update_marketplace', instanceId, { request }),
    );
  },

  refreshMarketplace: async (instanceId, marketplace) => {
    await runMarketplaceLifecycle(
      set,
      get,
      instanceId,
      { kind: 'refresh', target: marketplace },
      () => invokeMarketplace('refresh_marketplace', instanceId, { marketplace }),
    );
  },

  removeMarketplace: async (instanceId, marketplace) => {
    await runMarketplaceLifecycle(
      set,
      get,
      instanceId,
      { kind: 'remove', target: marketplace },
      () => invokeMarketplace('remove_marketplace', instanceId, { marketplace }),
    );
  },

  installPlugin: async (instanceId, request) => {
    await runMarketplaceLifecycle(
      set,
      get,
      instanceId,
      { kind: 'install', target: request.plugin },
      () => invokeMarketplace('install_plugin', instanceId, { request }),
    );
  },

  enablePlugin: async (instanceId, request) => {
    await runMarketplaceLifecycle(
      set,
      get,
      instanceId,
      { kind: 'enable', target: request.plugin },
      () => invokeMarketplace('enable_plugin', instanceId, { request }),
    );
  },

  disablePlugin: async (instanceId, request) => {
    await runMarketplaceLifecycle(
      set,
      get,
      instanceId,
      { kind: 'disable', target: request.plugin },
      () => invokeMarketplace('disable_plugin', instanceId, { request }),
    );
  },

  uninstallPlugin: async (instanceId, request) => {
    await runMarketplaceLifecycle(
      set,
      get,
      instanceId,
      { kind: 'uninstall', target: request.plugin },
      () => invokeMarketplace('uninstall_plugin', instanceId, { request }),
    );
  },

  reset: () => set(initialState),
}));

async function runMarketplaceLifecycle(
  set: (partial: Partial<SkillState> | ((state: SkillState) => Partial<SkillState>)) => void,
  get: () => SkillState,
  instanceId: string,
  operation: Omit<MarketplaceOperation, 'id'>,
  action: () => Promise<unknown>,
) {
  const existingOperation = get().recordsByInstanceId[instanceId]?.marketplaceOperation;
  if (existingOperation) {
    throw new Error(
      `Marketplace operation already in progress: ${existingOperation.kind} ${existingOperation.target}`,
    );
  }
  const requestId = ++navigationRequestSequence;
  const activeOperation: MarketplaceOperation = { id: requestId, ...operation };
  setInstanceRecord(set, instanceId, (record) => ({
    ...record,
    marketplaceRequestId: requestId,
    loadingMarketplace: true,
    marketplaceOperation: activeOperation,
    marketplaceError: null,
  }));
  try {
    await action();
    if (!isCurrentMarketplaceOperation(get(), instanceId, requestId)) return;
    if (get().activeInstanceId === instanceId) {
      await get().fetchMarketplaceGovernance(instanceId);
    }
    if (
      isCurrentMarketplaceOperation(get(), instanceId, requestId)
      && get().activeInstanceId === instanceId
    ) {
      await get().fetchSkills(instanceId);
    }
  } catch (e) {
    if (isCurrentMarketplaceOperation(get(), instanceId, requestId)) {
      setInstanceRecord(set, instanceId, (record) => ({
        ...record,
        marketplaceError: isRuntimeInputCancelledError(e) ? null : formatInvokeError(e),
      }), get().activeInstanceId === instanceId);
    }
    throw e;
  } finally {
    if (isCurrentMarketplaceOperation(get(), instanceId, requestId)) {
      setInstanceRecord(set, instanceId, (record) => ({
        ...record,
        loadingMarketplace: false,
        marketplaceOperation: null,
      }), get().activeInstanceId === instanceId);
    }
  }
}

function isCurrentMarketplaceOperation(
  state: SkillState,
  instanceId: string,
  operationId: number,
) {
  return state.recordsByInstanceId[instanceId]?.marketplaceOperation?.id === operationId;
}
