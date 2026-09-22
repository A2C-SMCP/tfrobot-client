import { fireEvent } from '@testing-library/react';
import { invoke } from '@tauri-apps/api/core';
import { NoticeBar } from '@/components/common/NoticeBar';
import { useNoticeLifecycle } from '@/components/common/useNoticeLifecycle';
import { IMPRESSION_FLUSH_THRESHOLD, useUiNoticeStore } from '@/stores/uiNoticeStore';
import { render, screen, waitFor } from '../helpers/render';

const mockedInvoke = vi.mocked(invoke);

/** One mounted notice. `enabled` stands in for a caller's own "should be on screen" conditions. */
function Notice({ enabled }: { enabled: boolean }) {
  const notice = useNoticeLifecycle('desktop-enumeration-unverified', enabled);
  if (!notice.visible) return <span>hidden</span>;
  return (
    <NoticeBar dismiss={{ label: 'Dismiss', onClick: notice.dismiss }}>
      Caveat
    </NoticeBar>
  );
}

function MountMany({ count, enabled }: { count: number; enabled: boolean }) {
  return (
    <>
      {Array.from({ length: count }, (_, index) => (
        <Notice key={index} enabled={enabled} />
      ))}
    </>
  );
}

function NoticeWithHelp({ onOpen }: { onOpen: () => void }) {
  const notice = useNoticeLifecycle('desktop-enumeration-unverified');
  if (!notice.visible) return <span>hidden</span>;
  return (
    <NoticeBar
      help={<button onClick={() => notice.openHelp(onOpen)}>Help</button>}
      dismiss={{ label: 'Dismiss', onClick: notice.dismiss }}
    >
      Caveat
    </NoticeBar>
  );
}

describe('useNoticeLifecycle', () => {
  beforeEach(() => {
    useUiNoticeStore.getState().reset();
    mockedInvoke.mockReset();
    mockedInvoke.mockResolvedValue({ schemaVersion: 1, entries: {} });
  });

  it('does not count an impression for a notice the caller is not showing', () => {
    useUiNoticeStore.setState({ loaded: true, entries: {} });

    render(<MountMany count={IMPRESSION_FLUSH_THRESHOLD} enabled={false} />);

    expect(screen.getAllByText('hidden')).toHaveLength(IMPRESSION_FLUSH_THRESHOLD);
    expect(mockedInvoke).not.toHaveBeenCalled();
  });

  it('counts impressions for rendered notices and flushes them in one write', async () => {
    useUiNoticeStore.setState({ loaded: true, entries: {} });

    render(<MountMany count={IMPRESSION_FLUSH_THRESHOLD} enabled />);

    await waitFor(() => expect(mockedInvoke).toHaveBeenCalledWith('update_ui_notice_state', {
      updates: {
        'desktop-enumeration-unverified': {
          impressionsDelta: IMPRESSION_FLUSH_THRESHOLD,
          helpClicksDelta: 0,
        },
      },
    }));
    expect(mockedInvoke).toHaveBeenCalledTimes(1);
  });

  it('hides the bar and persists the dismissal when the user turns it off', async () => {
    useUiNoticeStore.setState({ loaded: true, entries: {} });
    render(<Notice enabled />);

    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));

    expect(useUiNoticeStore.getState().isDismissed('desktop-enumeration-unverified')).toBe(true);
    await waitFor(() => expect(screen.queryByRole('note')).not.toBeInTheDocument());
    expect(mockedInvoke).toHaveBeenCalledWith('update_ui_notice_state', {
      updates: {
        'desktop-enumeration-unverified': expect.objectContaining({ dismissed: true }),
      },
    });
  });

  it('counts a help click and keeps the notice on screen when the user opens the help page', async () => {
    useUiNoticeStore.setState({ loaded: true, entries: {} });
    const onOpen = vi.fn();
    render(<NoticeWithHelp onOpen={onOpen} />);

    fireEvent.click(screen.getByRole('button', { name: 'Help' }));

    expect(onOpen).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('note')).toBeInTheDocument();
    await waitFor(() => expect(mockedInvoke).toHaveBeenCalledWith('update_ui_notice_state', {
      updates: {
        'desktop-enumeration-unverified': { impressionsDelta: 1, helpClicksDelta: 1 },
      },
    }));
  });
});
