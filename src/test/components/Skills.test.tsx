import { render, screen, fireEvent } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { Skills } from '@/components/Skills';
import type { SkillInfo, SkillSyncSummary } from '@/stores/skillsStore';

const mocks = vi.hoisted(() => ({
  fetchSkills: vi.fn(),
  syncSkills: vi.fn(),
  openSkillsRoot: vi.fn(),
  openSkillFolder: vi.fn(),
  openSkillMarkdownFile: vi.fn(),
  selectSkill: vi.fn(),
  store: {
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
  },
}));

vi.mock('@/stores/skillsStore', () => ({
  useSkillsStore: vi.fn(() => ({
    ...mocks.store,
    fetchSkills: mocks.fetchSkills,
    syncSkills: mocks.syncSkills,
    openSkillsRoot: mocks.openSkillsRoot,
    openSkillFolder: mocks.openSkillFolder,
    openSkillMarkdownFile: mocks.openSkillMarkdownFile,
    selectSkill: mocks.selectSkill,
  })),
}));

const mockSkills: SkillInfo[] = [
  {
    name: 'alpha',
    path: '/tmp/skills/alpha',
    skill_md_path: '/tmp/skills/alpha/SKILL.md',
    has_skill_md: true,
    is_skill: true,
    description: 'Alpha skill description',
    source: 'local',
  },
  {
    name: 'mcp:tfrobot-tools:image-gen',
    path: '/tmp/runtime-skill-home/mcp/tfrobot-tools/image-gen',
    skill_md_path: '/tmp/runtime-skill-home/mcp/tfrobot-tools/image-gen/SKILL.md',
    has_skill_md: true,
    is_skill: true,
    description: 'Image generation',
    source: 'mcp:tfrobot-tools',
    uri: 'skill://tfrobot-tools/image-gen',
  },
];

describe('Skills', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.store.skills = [];
    mocks.store.syncSummary = null;
    mocks.store.loading = false;
    mocks.store.syncing = false;
    mocks.store.opening = false;
    mocks.store.error = null;
    mocks.store.rootPath = null;
    mocks.store.selectedSkillPath = null;
    mocks.store.markdownByPath = {};
    mocks.store.previewLoading = false;
    mocks.store.previewError = null;
    mocks.fetchSkills.mockResolvedValue(undefined);
    mocks.syncSkills.mockResolvedValue(undefined);
    mocks.openSkillsRoot.mockResolvedValue(undefined);
    mocks.openSkillFolder.mockResolvedValue(undefined);
    mocks.openSkillMarkdownFile.mockResolvedValue(undefined);
    mocks.selectSkill.mockResolvedValue(undefined);
  });

  it('fetches skills on mount', () => {
    render(<Skills />);

    expect(mocks.fetchSkills).toHaveBeenCalled();
  });

  it('syncs skills on explicit sync action', () => {
    render(<Skills />);

    fireEvent.click(screen.getByRole('button', { name: /sync/i }));

    expect(mocks.syncSkills).toHaveBeenCalled();
  });

  it('renders empty state', () => {
    render(<Skills />);

    expect(screen.getByText('No skills found')).toBeInTheDocument();
  });

  it('renders skills table', () => {
    mocks.store.skills = mockSkills;

    render(<Skills />);

    expect(screen.getByText('alpha')).toBeInTheDocument();
    expect(screen.getByText('Alpha skill description')).toBeInTheDocument();
    expect(screen.getByText('local')).toBeInTheDocument();
    expect(screen.getByText('mcp:tfrobot-tools:image-gen')).toBeInTheDocument();
    expect(screen.getByText('Image generation')).toBeInTheDocument();
    expect(screen.getByText('mcp:tfrobot-tools')).toBeInTheDocument();
  });

  it('does not render status management UI', () => {
    mocks.store.skills = mockSkills;

    render(<Skills />);

    expect(screen.queryByText('Enable')).not.toBeInTheDocument();
    expect(screen.queryByText('Disable')).not.toBeInTheDocument();
  });

  it('does not render a separate refresh action', () => {
    render(<Skills />);

    expect(screen.queryByText('Refresh')).not.toBeInTheDocument();
  });

  it('opens skills root', () => {
    render(<Skills />);

    fireEvent.click(screen.getByText('Open Root Folder'));

    expect(mocks.openSkillsRoot).toHaveBeenCalled();
  });

  it('opens a skill folder from row actions', () => {
    mocks.store.skills = mockSkills;

    render(<Skills />);

    fireEvent.click(screen.getAllByText('Folder')[0]);

    expect(mocks.openSkillFolder).toHaveBeenCalledWith(mockSkills[0]);
  });

  it('does not expose folder action for MCP skills', () => {
    mocks.store.skills = mockSkills;

    render(<Skills />);

    expect(screen.getAllByText('Folder')).toHaveLength(1);
  });

  it('selects a skill when preview action is clicked', () => {
    mocks.store.skills = mockSkills;

    render(<Skills />);

    fireEvent.click(screen.getAllByText('Preview')[0]);

    expect(mocks.selectSkill).toHaveBeenCalledWith(mockSkills[0]);
  });

  it('renders markdown preview for selected skill', () => {
    mocks.store.skills = mockSkills;
    mocks.store.selectedSkillPath = '/tmp/skills/alpha';
    mocks.store.markdownByPath = {
      '/tmp/skills/alpha': '# Alpha\n\n- Uses `SKILL.md`\n\n```bash\npnpm test\n```',
    };

    render(<Skills />);

    fireEvent.click(screen.getAllByText('Preview')[0]);

    expect(screen.getByRole('heading', { name: 'Alpha' })).toBeInTheDocument();
    expect(screen.getByText(/Uses/)).toBeInTheDocument();
    expect(screen.getByText('SKILL.md')).toBeInTheDocument();
    expect(screen.getByText('pnpm test')).toBeInTheDocument();
  });

  it('renders missing SKILL.md preview state', () => {
    mocks.store.skills = mockSkills;
    mocks.store.selectedSkillPath = '/tmp/skills/alpha';
    mocks.store.previewError = 'missing_skill_md';

    render(<Skills />);

    fireEvent.click(screen.getAllByText('Preview')[0]);

    expect(screen.getByText('This skill does not have a SKILL.md file')).toBeInTheDocument();
  });

  it('renders preview read error', () => {
    mocks.store.skills = mockSkills;
    mocks.store.selectedSkillPath = '/tmp/skills/alpha';
    mocks.store.previewError = 'read failed';

    render(<Skills />);

    fireEvent.click(screen.getAllByText('Preview')[0]);

    expect(screen.getByText('Failed to load SKILL.md')).toBeInTheDocument();
    expect(screen.getByText('read failed')).toBeInTheDocument();
  });

  it('renders empty markdown preview state', () => {
    mocks.store.skills = mockSkills;
    mocks.store.selectedSkillPath = '/tmp/skills/alpha';
    mocks.store.markdownByPath = { '/tmp/skills/alpha': '   ' };

    render(<Skills />);

    fireEvent.click(screen.getAllByText('Preview')[0]);

    expect(screen.getByText('SKILL.md is empty')).toBeInTheDocument();
  });

  it('renders error alert', () => {
    mocks.store.error = 'Skills root directory does not exist';

    render(<Skills />);

    expect(screen.getByText('Skills root directory does not exist')).toBeInTheDocument();
  });

  it('renders skill sync conflicts', () => {
    mocks.store.syncSummary = {
      local_synced: 1,
      mcp_synced: 0,
      ignored_conflicts: [
        {
          skill_name: 'code-review',
          kept_source: 'user',
          kept_name: 'code-review',
          ignored_source: 'user',
          ignored_name: 'code-review',
          reason: 'same full protocol skill name; keeping first loaded',
        },
      ],
      skipped: [],
    };

    render(<Skills />);

    expect(screen.getByText('Skill sync conflicts')).toBeInTheDocument();
    expect(screen.getByText(/kept code-review/)).toBeInTheDocument();
  });

  it('renders skill sync skipped entries and summary counts', () => {
    mocks.store.syncSummary = {
      local_synced: 2,
      mcp_synced: 1,
      ignored_conflicts: [],
      skipped: [
        {
          skill_name: 'body-only',
          source: 'user',
          reason: 'missing frontmatter description',
        },
      ],
    };

    render(<Skills />);

    expect(screen.getByText('Skill sync summary')).toBeInTheDocument();
    expect(screen.getByText('Local synced: 2')).toBeInTheDocument();
    expect(screen.getByText('MCP synced: 1')).toBeInTheDocument();
    expect(screen.getByText('Skipped: 1')).toBeInTheDocument();
    expect(screen.getByText('Conflicts: 0')).toBeInTheDocument();
    expect(screen.getByText('Skipped skills')).toBeInTheDocument();
    expect(screen.getByText(/body-only/)).toBeInTheDocument();
    expect(screen.getByText(/missing frontmatter description/)).toBeInTheDocument();
  });

  it('renders skill sync conflicts and skipped entries together', () => {
    mocks.store.syncSummary = {
      local_synced: 0,
      mcp_synced: 0,
      ignored_conflicts: [
        {
          skill_name: 'code-review',
          kept_source: 'marketplace',
          kept_name: 'code-review',
          ignored_source: 'user',
          ignored_name: 'code-review',
          reason: 'same full protocol skill name; keeping first loaded',
        },
      ],
      skipped: [
        {
          skill_name: 'body-only',
          source: 'user',
          reason: 'missing frontmatter description',
        },
      ],
    };

    render(<Skills />);

    expect(screen.getByText('Skill sync conflicts')).toBeInTheDocument();
    expect(screen.getByText('Skipped skills')).toBeInTheDocument();
  });

  it('shows loading state', () => {
    mocks.store.loading = true;

    render(<Skills />);

    expect(document.querySelector('.ant-spin')).toBeInTheDocument();
  });
});
