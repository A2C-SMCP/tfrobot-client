import { act, render, screen, within } from '../helpers/render';
import { beforeEach, describe, expect, it } from 'vitest';
import { NoticeStats } from '@/components/DebugPanel/NoticeStats';
import { useUiNoticeStore } from '@/stores/uiNoticeStore';

describe('NoticeStats', () => {
  beforeEach(() => {
    useUiNoticeStore.getState().reset();
  });

  it('shows buffered impressions immediately without requiring a disk flush', () => {
    useUiNoticeStore.setState({ loaded: true, entries: {} });
    render(<NoticeStats />);
    act(() => useUiNoticeStore.getState().recordImpression('mcp-runtime-keychain'));
    const row = screen.getByText('MCP runtime keychain access').closest('tr')!;
    expect(within(row).getByText('1')).toBeInTheDocument();
  });

  it('lists every notice even before any of them has been shown', () => {
    useUiNoticeStore.setState({ loaded: true, entries: {} });

    render(<NoticeStats />);

    expect(screen.getByText('MCP runtime keychain access')).toBeInTheDocument();
    expect(screen.getByText('Password variable keychain access')).toBeInTheDocument();
    expect(screen.getByText('Remote control security')).toBeInTheDocument();
    expect(screen.getByText('Desktop enumeration unverified')).toBeInTheDocument();
    expect(screen.getByText(/never written to the activity log/)).toBeInTheDocument();
  });

  it('derives dismiss and help click rates from the local counters', () => {
    useUiNoticeStore.setState({
      loaded: true,
      entries: {
        'mcp-runtime-keychain': {
          firstSeenAt: 1,
          dismissedAt: 2,
          impressions: 4,
          helpClicks: 1,
        },
        'remote-control-security': {
          firstSeenAt: 1,
          dismissedAt: null,
          impressions: 0,
          helpClicks: 0,
        },
      },
    });

    render(<NoticeStats />);

    const mcpRow = screen.getByText('MCP runtime keychain access').closest('tr')!;
    expect(within(mcpRow).getByText('4')).toBeInTheDocument();
    expect(within(mcpRow).getByText('Yes')).toBeInTheDocument();
    // One dismissal and one help click across four impressions.
    expect(within(mcpRow).getAllByText('25%')).toHaveLength(2);

    const securityRow = screen.getByText('Remote control security').closest('tr')!;
    expect(within(securityRow).getByText('No')).toBeInTheDocument();
    // No impressions means the rates are unknowable rather than zero.
    expect(within(securityRow).getAllByText('—')).toHaveLength(2);
  });
});
