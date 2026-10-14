import { AutoUpdateCheck } from '@/components/ApplicationUpdate/AutoUpdateCheck';
import { render, waitFor } from '../helpers/render';
import {
  claimAutomaticUpdateCheck,
  recordUpdateActivity,
} from '@/services/applicationUpdater';

vi.mock('@/services/applicationUpdater', () => ({
  claimAutomaticUpdateCheck: vi.fn(),
  closeUpdate: vi.fn().mockResolvedValue(undefined),
  createUpdateCorrelationId: vi.fn(() => 'update-test'),
  deferUpdate: vi.fn().mockResolvedValue(undefined),
  recordUpdateActivity: vi.fn().mockResolvedValue(undefined),
}));

describe('AutoUpdateCheck', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(claimAutomaticUpdateCheck).mockResolvedValue(true);
  });

  it('does not check when the persisted 24-hour claim is unavailable', async () => {
    vi.mocked(claimAutomaticUpdateCheck).mockResolvedValue(false);

    render(<AutoUpdateCheck />);

    await waitFor(() => expect(claimAutomaticUpdateCheck).toHaveBeenCalledOnce());
    expect(recordUpdateActivity).not.toHaveBeenCalled();
  });
});
