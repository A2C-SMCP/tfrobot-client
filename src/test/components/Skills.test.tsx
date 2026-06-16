import { render, screen, fireEvent } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { Skills } from '@/components/Skills';
import type { SkillInfo } from '@/stores/skillsStore';

const mocks = vi.hoisted(() => ({
  fetchSkills: vi.fn(),
  openSkillsRoot: vi.fn(),
  openSkillFolder: vi.fn(),
  openSkillMarkdownFile: vi.fn(),
  selectSkill: vi.fn(),
  store: {
    skills: [] as SkillInfo[],
    loading: false,
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
    description: 'Alpha skill description',
    source: 'local',
  },
  {
    name: 'beta',
    path: '/tmp/skills/beta',
    skill_md_path: '/tmp/skills/beta/SKILL.md',
    has_skill_md: false,
    description: null,
    source: 'local',
  },
];

describe('Skills', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.store.skills = [];
    mocks.store.loading = false;
    mocks.store.opening = false;
    mocks.store.error = null;
    mocks.store.rootPath = null;
    mocks.store.selectedSkillPath = null;
    mocks.store.markdownByPath = {};
    mocks.store.previewLoading = false;
    mocks.store.previewError = null;
    mocks.fetchSkills.mockResolvedValue(undefined);
    mocks.openSkillsRoot.mockResolvedValue(undefined);
    mocks.openSkillFolder.mockResolvedValue(undefined);
    mocks.openSkillMarkdownFile.mockResolvedValue(undefined);
    mocks.selectSkill.mockResolvedValue(undefined);
  });

  it('fetches skills on mount', () => {
    render(<Skills />);

    expect(mocks.fetchSkills).toHaveBeenCalled();
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
    expect(screen.getAllByText('Local').length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText('beta')).toBeInTheDocument();
    expect(screen.getByText('No description')).toBeInTheDocument();
  });

  it('does not render status management UI', () => {
    mocks.store.skills = mockSkills;

    render(<Skills />);

    expect(screen.queryByText('Enable')).not.toBeInTheDocument();
    expect(screen.queryByText('Disable')).not.toBeInTheDocument();
  });

  it('refreshes skills', () => {
    render(<Skills />);

    fireEvent.click(screen.getByText('Refresh'));

    expect(mocks.fetchSkills).toHaveBeenCalledTimes(2);
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
    mocks.store.selectedSkillPath = '/tmp/skills/beta';
    mocks.store.previewError = 'missing_skill_md';

    render(<Skills />);

    fireEvent.click(screen.getAllByText('Preview')[1]);

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

  it('shows loading state', () => {
    mocks.store.loading = true;

    render(<Skills />);

    expect(document.querySelector('.ant-spin')).toBeInTheDocument();
  });
});
