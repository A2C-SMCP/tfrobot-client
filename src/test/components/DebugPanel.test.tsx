import { render, screen } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { DebugPanel } from '@/components/DebugPanel';
import { ScopeContext } from '@/components/Navigation/navigationMemoryState';

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
    render(<DebugPanel instanceId="computer-a" />);
    expect(screen.getByText('Tools')).toBeInTheDocument();
    expect(screen.getByText('Resources')).toBeInTheDocument();
    expect(screen.getByText('History')).toBeInTheDocument();
    expect(screen.getAllByRole('tab')).toHaveLength(3);
  });

  it('renders ToolBrowser as default active tab', () => {
    render(<DebugPanel instanceId="computer-a" />);
    expect(screen.getByTestId('tool-browser')).toBeInTheDocument();
  });

  it('restores Tools when the remembered tab has been removed', () => {
    render(
      <ScopeContext.Provider value={new Map([['debug.tab', 'notices']])}>
        <DebugPanel instanceId="computer-a" />
      </ScopeContext.Provider>,
    );
    expect(screen.getByRole('tab', { name: 'Tools', selected: true })).toBeInTheDocument();
    expect(screen.getByTestId('tool-browser')).toBeInTheDocument();
  });
});
