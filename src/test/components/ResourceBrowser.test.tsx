import { render, screen, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { ResourceBrowser } from '@/components/DebugPanel/ResourceBrowser';
import { useMcpStore } from '@/stores/mcpStore';

interface MockMcpStore {
  servers: Array<{ bundleId: string; name: string; running: boolean; disabled: boolean; status_message: string }>;
  loading: boolean;
  activeInstanceId: string | null;
  fetchServers: ReturnType<typeof vi.fn<() => Promise<void>>>;
}

interface MockDebugStore {
  resources: Array<{
    server: string;
    uri: string;
    name: string;
    description?: string;
    mime_type?: string;
  }>;
  resourcesLoading: boolean;
  resourcesNextCursor: string | null;
  error: string | null;
  fetchResources: ReturnType<typeof vi.fn>;
}

let mockMcpStore = makeMcpStore();
let mockDebugStore = makeDebugStore();

function makeMcpStore(overrides?: Partial<MockMcpStore>): MockMcpStore {
  return {
    servers: [
      { bundleId: 'fs-bundle', name: 'fs-server', running: true, disabled: false, status_message: 'Running' },
      { bundleId: 'stopped-bundle', name: 'stopped-server', running: false, disabled: false, status_message: 'Stopped' },
    ],
    loading: false,
    activeInstanceId: 'computer-a',
    fetchServers: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}

function makeDebugStore(overrides?: Partial<MockDebugStore>): MockDebugStore {
  return {
    resources: [
      {
        server: 'fs-server',
        uri: 'file://readme',
        name: 'README.md',
        description: 'Project readme',
        mime_type: 'text/markdown',
      },
    ],
    resourcesLoading: false,
    resourcesNextCursor: null,
    error: null,
    fetchResources: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}

vi.mock('@/stores/mcpStore', () => ({
  useMcpStore: vi.fn(() => mockMcpStore),
}));

vi.mock('@/stores/debugStore', () => ({
  useDebugStore: vi.fn(() => mockDebugStore),
}));

describe('ResourceBrowser', () => {
  beforeEach(() => {
    mockMcpStore = makeMcpStore();
    mockDebugStore = makeDebugStore();
    vi.mocked(useMcpStore).mockImplementation(() => mockMcpStore as ReturnType<typeof useMcpStore>);
    vi.clearAllMocks();
  });

  it('fetches resources for the current computer and first running server', async () => {
    render(<ResourceBrowser instanceId="computer-a" />);

    await waitFor(() => {
      expect(mockMcpStore.fetchServers).toHaveBeenCalledWith('computer-a');
      expect(mockDebugStore.fetchResources).toHaveBeenCalledWith('computer-a', 'fs-bundle');
    });

    expect(screen.getByText('README.md')).toBeInTheDocument();
    expect(screen.getByText('file://readme')).toBeInTheDocument();
    expect(screen.getByText('text/markdown')).toBeInTheDocument();
    expect(screen.getByText('Computer: computer-a')).toBeInTheDocument();
  });

  it('does not request new instance resources with a stale server after instance switch', async () => {
    const { rerender } = render(<ResourceBrowser instanceId="computer-a" />);

    await waitFor(() => {
      expect(mockDebugStore.fetchResources).toHaveBeenCalledWith('computer-a', 'fs-bundle');
    });

    mockMcpStore = makeMcpStore({
      activeInstanceId: 'computer-a',
      fetchServers: vi.fn().mockResolvedValue(undefined),
    });
    vi.mocked(useMcpStore).mockImplementation(() => mockMcpStore as ReturnType<typeof useMcpStore>);

    rerender(<ResourceBrowser instanceId="computer-b" />);

    await waitFor(() => {
      expect(mockMcpStore.fetchServers).toHaveBeenCalledWith('computer-b');
    });

    expect(mockDebugStore.fetchResources).not.toHaveBeenCalledWith(
      'computer-b',
      'fs-bundle',
    );
  });
});
