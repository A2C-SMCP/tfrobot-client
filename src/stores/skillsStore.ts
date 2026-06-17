import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';

export interface SkillInfo {
  name: string;
  path: string;
  skill_md_path: string;
  has_skill_md: boolean;
  is_skill: boolean;
  invalid_reason?: string | null;
  description?: string | null;
  source: 'local' | 'mcp' | string;
  uri?: string | null;
}

export interface SkillConflict {
  skill_name: string;
  kept_source: string;
  kept_name: string;
  ignored_source: string;
  ignored_name: string;
  reason: string;
}

export interface SkillSkipped {
  skill_name: string;
  source: string;
  reason: string;
}

export interface SkillSyncSummary {
  local_synced: number;
  mcp_synced: number;
  ignored_conflicts: SkillConflict[];
  skipped: SkillSkipped[];
}

interface SkillsState {
  skills: SkillInfo[];
  syncSummary: SkillSyncSummary | null;
  loading: boolean;
  syncing: boolean;
  opening: boolean;
  error: string | null;
  rootPath: string | null;
  selectedSkillPath: string | null;
  markdownByPath: Record<string, string>;
  previewLoading: boolean;
  previewError: string | null;

  fetchSkills: () => Promise<void>;
  syncSkills: () => Promise<void>;
  openSkillsRoot: () => Promise<void>;
  openSkillFolder: (skill: SkillInfo) => Promise<void>;
  openSkillMarkdownFile: (skill: SkillInfo) => Promise<void>;
  selectSkill: (skill: SkillInfo) => Promise<void>;
  fetchSkillMarkdown: (skill: SkillInfo) => Promise<void>;
  reset: () => void;
}

const initialState = {
  skills: [] as SkillInfo[],
  syncSummary: null as SkillSyncSummary | null,
  loading: false,
  syncing: false,
  opening: false,
  error: null as string | null,
  rootPath: null as string | null,
  selectedSkillPath: null as string | null,
  markdownByPath: {} as Record<string, string>,
  previewLoading: false,
  previewError: null as string | null,
};

export const useSkillsStore = create<SkillsState>((set, get) => ({
  ...initialState,

  reset: () => set(initialState),

  fetchSkills: async () => {
    set({ loading: true, error: null });
    try {
      const skills = await invoke<SkillInfo[]>('list_skills');
      const selectedSkillPath = get().selectedSkillPath;
      set({
        skills,
        loading: false,
        selectedSkillPath: skills.some((skill) => skill.path === selectedSkillPath)
          ? selectedSkillPath
          : null,
      });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  syncSkills: async () => {
    set({ syncing: true, error: null });
    try {
      const syncSummary = await invoke<SkillSyncSummary>('refresh_skill_sync_summary');
      const skills = await invoke<SkillInfo[]>('list_skills');
      const selectedSkillPath = get().selectedSkillPath;
      set({
        skills,
        syncSummary,
        syncing: false,
        selectedSkillPath: skills.some((skill) => skill.path === selectedSkillPath)
          ? selectedSkillPath
          : null,
      });
    } catch (e) {
      set({ error: String(e), syncing: false });
    }
  },

  openSkillsRoot: async () => {
    set({ opening: true, error: null });
    try {
      const rootPath = await invoke<string>('open_skills_root');
      set({ rootPath, opening: false });
    } catch (e) {
      set({ error: String(e), opening: false });
      throw e;
    }
  },

  openSkillFolder: async (skill: SkillInfo) => {
    set({ opening: true, error: null });
    try {
      const rootPath = await invoke<string>('open_skill_folder', {
        skillPath: skill.path,
      });
      set({ rootPath, opening: false });
    } catch (e) {
      set({ error: String(e), opening: false });
      throw e;
    }
  },

  openSkillMarkdownFile: async (skill: SkillInfo) => {
    set({ opening: true, error: null });
    try {
      const rootPath = await invoke<string>('open_skill_markdown_file', {
        skillPath: skill.path,
      });
      set({ rootPath, opening: false });
    } catch (e) {
      set({ error: String(e), opening: false });
      throw e;
    }
  },

  selectSkill: async (skill: SkillInfo) => {
    set({ selectedSkillPath: skill.path, previewError: null });
    if (!skill.has_skill_md) {
      set({ previewLoading: false, previewError: 'missing_skill_md' });
      return;
    }
    await get().fetchSkillMarkdown(skill);
  },

  fetchSkillMarkdown: async (skill: SkillInfo) => {
    if (!skill.has_skill_md) {
      set({ selectedSkillPath: skill.path, previewLoading: false, previewError: 'missing_skill_md' });
      return;
    }

    const cached = get().markdownByPath[skill.path];
    if (cached !== undefined) {
      set({ selectedSkillPath: skill.path, previewLoading: false, previewError: null });
      return;
    }

    set({ selectedSkillPath: skill.path, previewLoading: true, previewError: null });
    try {
      const markdown = await invoke<string>('read_skill_markdown', {
        skillPath: skill.path,
      });
      set((state) => ({
        markdownByPath: { ...state.markdownByPath, [skill.path]: markdown },
        previewLoading: state.selectedSkillPath === skill.path ? false : state.previewLoading,
      }));
    } catch (e) {
      if (get().selectedSkillPath === skill.path) {
        set({ previewError: String(e), previewLoading: false });
      }
    }
  },
}));
