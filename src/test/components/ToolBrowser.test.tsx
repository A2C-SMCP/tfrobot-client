import { render, screen, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { ToolBrowser } from '@/components/DebugPanel/ToolBrowser';

const mockDebugStore = {
  tools: [
    {
      name: 'click',
      description: 'Click target',
      inputSchema: {},
      server: 'desktop',
    },
  ],
  toolsLoading: false,
  selectedTool: {
    name: 'click',
    description: 'Click target',
    inputSchema: {},
    server: 'desktop',
  },
  fetchTools: vi.fn().mockResolvedValue(undefined),
  selectTool: vi.fn(),
};

vi.mock('@/stores/debugStore', () => ({
  useDebugStore: vi.fn(() => mockDebugStore),
}));

vi.mock('@/components/DebugPanel/ToolCallTest', () => ({
  ToolCallTest: () => <div data-testid="tool-call-test" />,
}));

describe('ToolBrowser', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockDebugStore.tools = [
      {
        name: 'click',
        description: 'Click target',
        inputSchema: {},
        server: 'desktop',
      },
    ];
    mockDebugStore.selectedTool = {
      name: 'click',
      description: 'Click target',
      inputSchema: {},
      server: 'desktop',
    };
  });

  it('clears selected tool and refetches tools when computer instance changes', async () => {
    const { rerender } = render(<ToolBrowser instanceId="computer-a" />);

    await waitFor(() => {
      expect(mockDebugStore.selectTool).toHaveBeenCalledWith(null);
      expect(mockDebugStore.fetchTools).toHaveBeenCalledWith('computer-a');
    });

    rerender(<ToolBrowser instanceId="computer-b" />);

    await waitFor(() => {
      expect(mockDebugStore.selectTool).toHaveBeenCalledTimes(2);
      expect(mockDebugStore.fetchTools).toHaveBeenCalledWith('computer-b');
    });
  });

  it('renders known server names for tools', () => {
    render(<ToolBrowser instanceId="computer-a" />);

    expect(screen.getAllByText('click').length).toBeGreaterThan(0);
    expect(screen.getAllByText('desktop').length).toBeGreaterThan(0);
  });

  it('hides unknown server labels and server filter', () => {
    mockDebugStore.tools = [
      {
        name: 'echo',
        description: 'Echoes input',
        inputSchema: {},
        server: 'unknown',
      },
    ];
    mockDebugStore.selectedTool = {
      name: 'echo',
      description: 'Echoes input',
      inputSchema: {},
      server: 'unknown',
    };

    render(<ToolBrowser instanceId="computer-a" />);

    expect(screen.getAllByText('echo').length).toBeGreaterThan(0);
    expect(screen.queryByText('unknown')).not.toBeInTheDocument();
    expect(screen.queryByText('All Servers')).not.toBeInTheDocument();
  });
});
