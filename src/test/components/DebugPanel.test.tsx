import { render, screen } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { DebugPanel } from '@/components/DebugPanel';

// Mock all sub-components to isolate DebugPanel
vi.mock('@/components/DebugPanel/ToolBrowser', () => ({
  ToolBrowser: () => <div data-testid="tool-browser">ToolBrowser</div>,
}));
vi.mock('@/components/DebugPanel/CallHistory', () => ({
  CallHistory: () => <div data-testid="call-history">CallHistory</div>,
}));
vi.mock('@/components/DebugPanel/ResourceBrowser', () => ({
  ResourceBrowser: () => <div data-testid="resource-browser">ResourceBrowser</div>,
}));

describe('DebugPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders tabs', () => {
    render(<DebugPanel />);
    expect(screen.getByText('Tools')).toBeInTheDocument();
    expect(screen.getByText('Resources')).toBeInTheDocument();
    expect(screen.getByText('History')).toBeInTheDocument();
  });

  it('renders ToolBrowser as default active tab', () => {
    render(<DebugPanel />);
    expect(screen.getByTestId('tool-browser')).toBeInTheDocument();
  });
});
