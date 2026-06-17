import { invoke } from '@tauri-apps/api/core';
import { useSkillsStore, type SkillInfo, type SkillSyncSummary } from '@/stores/skillsStore';

const mockedInvoke = vi.mocked(invoke);

const mockSkills: SkillInfo[] = [
  {
    name: 'demo-skill',
    path: '/tmp/skills/demo-skill',
    skill_md_path: '/tmp/skills/demo-skill/SKILL.md',
    has_skill_md: true,
    is_skill: true,
    description: 'Demo skill description',
    source: 'local',
  },
];

const emptySummary: SkillSyncSummary = {
  local_synced: 0,
  mcp_synced: 0,
  ignored_conflicts: [],
  skipped: [],
};

const missingSkillMd: SkillInfo = {
  name: 'missing-md',
  path: '/tmp/skills/missing-md',
  skill_md_path: '/tmp/skills/missing-md/SKILL.md',
  has_skill_md: false,
  is_skill: false,
  invalid_reason: 'missing SKILL.md',
  description: null,
  source: 'local',
};

const otherSkill: SkillInfo = {
  name: 'other-skill',
  path: '/tmp/skills/other-skill',
  skill_md_path: '/tmp/skills/other-skill/SKILL.md',
  has_skill_md: true,
  is_skill: true,
  description: 'Other skill description',
  source: 'local',
};

describe('skillsStore', () => {
  beforeEach(() => {
    useSkillsStore.getState().reset();
    mockedInvoke.mockReset();
  });

  describe('fetchSkills', () => {
    it('populates skills', async () => {
      mockedInvoke.mockImplementation((command) => {
        if (command === 'list_skills') {
          return Promise.resolve(mockSkills);
        }
        return Promise.reject(new Error(`Unexpected command: ${command}`));
      });

      await useSkillsStore.getState().fetchSkills();

      expect(mockedInvoke).toHaveBeenCalledWith('list_skills');
      expect(useSkillsStore.getState().skills).toEqual(mockSkills);
      expect(useSkillsStore.getState().syncSummary).toBeNull();
      expect(useSkillsStore.getState().loading).toBe(false);
    });

    it('sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('read error');

      await useSkillsStore.getState().fetchSkills();

      expect(useSkillsStore.getState().error).toBe('read error');
      expect(useSkillsStore.getState().loading).toBe(false);
    });
  });

  describe('syncSkills', () => {
    it('refreshes skills and runtime sync summary explicitly', async () => {
      mockedInvoke.mockImplementation((command) => {
        if (command === 'list_skills') {
          return Promise.resolve(mockSkills);
        }
        if (command === 'refresh_skill_sync_summary') {
          return Promise.resolve(emptySummary);
        }
        return Promise.reject(new Error(`Unexpected command: ${command}`));
      });

      await useSkillsStore.getState().syncSkills();

      expect(mockedInvoke).toHaveBeenCalledWith('list_skills');
      expect(mockedInvoke).toHaveBeenCalledWith('refresh_skill_sync_summary');
      expect(mockedInvoke.mock.calls[0][0]).toBe('refresh_skill_sync_summary');
      expect(mockedInvoke.mock.calls[1][0]).toBe('list_skills');
      expect(useSkillsStore.getState().skills).toEqual(mockSkills);
      expect(useSkillsStore.getState().syncSummary).toEqual(emptySummary);
      expect(useSkillsStore.getState().syncing).toBe(false);
    });

    it('sets error on explicit sync failure', async () => {
      mockedInvoke.mockRejectedValueOnce('sync error');

      await useSkillsStore.getState().syncSkills();

      expect(useSkillsStore.getState().error).toBe('sync error');
      expect(useSkillsStore.getState().syncing).toBe(false);
    });
  });

  describe('openSkillsRoot', () => {
    it('stores opened root path', async () => {
      mockedInvoke.mockResolvedValueOnce('/tmp/skills');

      await useSkillsStore.getState().openSkillsRoot();

      expect(mockedInvoke).toHaveBeenCalledWith('open_skills_root');
      expect(useSkillsStore.getState().rootPath).toBe('/tmp/skills');
      expect(useSkillsStore.getState().opening).toBe(false);
    });

    it('sets error and rethrows on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('open error');

      await expect(useSkillsStore.getState().openSkillsRoot()).rejects.toThrow();
      expect(useSkillsStore.getState().error).toBe('open error');
      expect(useSkillsStore.getState().opening).toBe(false);
    });
  });

  describe('openSkillFolder', () => {
    it('opens the selected skill folder', async () => {
      mockedInvoke.mockResolvedValueOnce('/tmp/skills/demo-skill');

      await useSkillsStore.getState().openSkillFolder(mockSkills[0]);

      expect(mockedInvoke).toHaveBeenCalledWith('open_skill_folder', {
        skillPath: '/tmp/skills/demo-skill',
      });
      expect(useSkillsStore.getState().rootPath).toBe('/tmp/skills/demo-skill');
      expect(useSkillsStore.getState().opening).toBe(false);
    });
  });

  describe('openSkillMarkdownFile', () => {
    it('opens the selected skill markdown file', async () => {
      mockedInvoke.mockResolvedValueOnce('/tmp/skills/demo-skill/SKILL.md');

      await useSkillsStore.getState().openSkillMarkdownFile(mockSkills[0]);

      expect(mockedInvoke).toHaveBeenCalledWith('open_skill_markdown_file', {
        skillPath: '/tmp/skills/demo-skill',
      });
      expect(useSkillsStore.getState().rootPath).toBe('/tmp/skills/demo-skill/SKILL.md');
      expect(useSkillsStore.getState().opening).toBe(false);
    });
  });

  describe('selectSkill', () => {
    it('loads and caches markdown content', async () => {
      mockedInvoke.mockResolvedValueOnce('# Demo');

      await useSkillsStore.getState().selectSkill(mockSkills[0]);

      expect(mockedInvoke).toHaveBeenCalledWith('read_skill_markdown', {
        skillPath: '/tmp/skills/demo-skill',
      });
      expect(useSkillsStore.getState().selectedSkillPath).toBe('/tmp/skills/demo-skill');
      expect(useSkillsStore.getState().markdownByPath['/tmp/skills/demo-skill']).toBe('# Demo');
      expect(useSkillsStore.getState().previewLoading).toBe(false);
      expect(useSkillsStore.getState().previewError).toBeNull();
    });

    it('uses cached markdown without invoking backend again', async () => {
      useSkillsStore.setState({
        markdownByPath: { '/tmp/skills/demo-skill': '# Cached' },
      });

      await useSkillsStore.getState().selectSkill(mockSkills[0]);

      expect(mockedInvoke).not.toHaveBeenCalled();
      expect(useSkillsStore.getState().selectedSkillPath).toBe('/tmp/skills/demo-skill');
    });

    it('sets missing status without invoking backend when SKILL.md is absent', async () => {
      await useSkillsStore.getState().selectSkill(missingSkillMd);

      expect(mockedInvoke).not.toHaveBeenCalled();
      expect(useSkillsStore.getState().selectedSkillPath).toBe('/tmp/skills/missing-md');
      expect(useSkillsStore.getState().previewError).toBe('missing_skill_md');
    });

    it('sets preview error on read failure', async () => {
      mockedInvoke.mockRejectedValueOnce('read failed');

      await useSkillsStore.getState().selectSkill(mockSkills[0]);

      expect(useSkillsStore.getState().previewError).toBe('read failed');
      expect(useSkillsStore.getState().previewLoading).toBe(false);
    });

    it('ignores stale preview errors after another skill is selected', async () => {
      let rejectFirstRead: (reason?: unknown) => void = () => {};
      mockedInvoke.mockImplementation((command, args) => {
        const skillPath =
          typeof args === 'object' && args !== null && !Array.isArray(args)
            ? (args as { skillPath?: string }).skillPath
            : undefined;
        if (command === 'read_skill_markdown' && skillPath === mockSkills[0].path) {
          return new Promise((_resolve, reject) => {
            rejectFirstRead = reject;
          });
        }
        if (command === 'read_skill_markdown' && skillPath === otherSkill.path) {
          return Promise.resolve('# Other');
        }
        return Promise.reject(new Error(`Unexpected command: ${command}`));
      });

      const firstSelection = useSkillsStore.getState().selectSkill(mockSkills[0]);
      await useSkillsStore.getState().selectSkill(otherSkill);
      rejectFirstRead('first read failed');
      await firstSelection;

      const state = useSkillsStore.getState();
      expect(state.selectedSkillPath).toBe(otherSkill.path);
      expect(state.markdownByPath[otherSkill.path]).toBe('# Other');
      expect(state.previewError).toBeNull();
      expect(state.previewLoading).toBe(false);
    });
  });

  it('resets state', () => {
    useSkillsStore.setState({
      skills: mockSkills,
      syncSummary: emptySummary,
      loading: true,
      syncing: true,
      opening: true,
      error: 'error',
      rootPath: '/tmp/skills',
      selectedSkillPath: '/tmp/skills/demo-skill',
      markdownByPath: { '/tmp/skills/demo-skill': '# Demo' },
      previewLoading: true,
      previewError: 'preview error',
    });

    useSkillsStore.getState().reset();

    expect(useSkillsStore.getState().skills).toEqual([]);
    expect(useSkillsStore.getState().syncSummary).toBeNull();
    expect(useSkillsStore.getState().loading).toBe(false);
    expect(useSkillsStore.getState().syncing).toBe(false);
    expect(useSkillsStore.getState().opening).toBe(false);
    expect(useSkillsStore.getState().error).toBeNull();
    expect(useSkillsStore.getState().rootPath).toBeNull();
    expect(useSkillsStore.getState().selectedSkillPath).toBeNull();
    expect(useSkillsStore.getState().markdownByPath).toEqual({});
    expect(useSkillsStore.getState().previewLoading).toBe(false);
    expect(useSkillsStore.getState().previewError).toBeNull();
  });
});
