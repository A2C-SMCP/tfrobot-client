import { render, waitFor } from '../helpers/render';
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
});
