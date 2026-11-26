import { render, screen, waitFor } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { ToolBrowser } from '@/components/DebugPanel/ToolBrowser';

const mockDebugStore = {
  activeInstanceId: 'computer-a',
  toolsLoadedInstanceId: 'computer-a',
  error: null,
  tools: [
    {
      name: 'click',
      displayName: 'click',
      description: 'Click target',
      inputSchema: {},
      server: 'desktop',
    },
  ],
  toolsLoading: false,
  selectedTool: {
    name: 'click',
    displayName: 'click',
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

const { ToolCallResultView } = await vi.importActual<
  typeof import('@/components/DebugPanel/ToolCallTest')
>('@/components/DebugPanel/ToolCallTest');

describe('ToolBrowser', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockDebugStore.tools = [
      {
        name: 'click',
        displayName: 'click',
        description: 'Click target',
        inputSchema: {},
        server: 'desktop',
      },
    ];
    mockDebugStore.selectedTool = {
      name: 'click',
      displayName: 'click',
      description: 'Click target',
      inputSchema: {},
      server: 'desktop',
    };
  });

  it('only exposes tools owned by the current computer when the instance changes', async () => {
    const { rerender } = render(<ToolBrowser instanceId="computer-a" />);

    await waitFor(() => {
      expect(mockDebugStore.fetchTools).toHaveBeenCalledWith('computer-a');
    });
    expect(screen.getByTestId('tool-call-test')).toBeVisible();

    rerender(<ToolBrowser instanceId="computer-b" />);

    await waitFor(() => {
      expect(mockDebugStore.fetchTools).toHaveBeenCalledWith('computer-b');
    });
    expect(screen.queryByTestId('tool-call-test')).not.toBeInTheDocument();
  });

  it('renders known server names for tools', () => {
    render(<ToolBrowser instanceId="computer-a" />);

    expect(screen.getAllByText('click').length).toBeGreaterThan(0);
    expect(screen.getAllByText('desktop').length).toBeGreaterThan(0);
  });

  it('keeps unattributed tools available in default and filtered views', () => {
    mockDebugStore.tools = [
      {
        name: 'echo',
        displayName: 'echo',
        description: 'Echoes input',
        inputSchema: {},
        server: 'unknown',
      },
    ];
    mockDebugStore.selectedTool = {
      name: 'echo',
      displayName: 'echo',
      description: 'Echoes input',
      inputSchema: {},
      server: 'unknown',
    };

    render(<ToolBrowser instanceId="computer-a" />);

    expect(screen.getAllByText('echo').length).toBeGreaterThan(0);
    expect(screen.queryByText('unknown')).not.toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Filter by MCP server' })).toBeInTheDocument();
  });

  it('renders OAuth authorization-required results from the SDK wire contract', () => {
    render(<ToolCallResultView response={{
      success: false,
      duration_ms: 12,
      result: {
        isError: true,
        _meta: { error_code: 4006, mcp_server: 'oauth-bundle' },
        content: [{ type: 'text', text: 'Authorization required' }],
      },
    }} />);

    expect(screen.getAllByText('Authorization required').length).toBeGreaterThan(0);
    expect(screen.getByText('Tool error code: 4006')).toBeInTheDocument();
  });
});
