import { render, screen } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { SmcpConnection } from '@/components/SmcpConnection';

const mockStore = {
  profiles: [],
  getStatus: vi.fn(() => ({ connected: false })),
  loading: false,
  error: null,
  fetchProfiles: vi.fn().mockResolvedValue(undefined),
  fetchStatus: vi.fn().mockResolvedValue(undefined),
  saveProfile: vi.fn().mockResolvedValue(undefined),
  deleteProfile: vi.fn().mockResolvedValue(undefined),
  connect: vi.fn().mockResolvedValue(undefined),
  disconnect: vi.fn().mockResolvedValue(undefined),
};

vi.mock('@/stores/connectionStore', () => ({
  useConnectionStore: vi.fn(() => mockStore),
}));

import { useConnectionStore } from '@/stores/connectionStore';
const mockUseConnectionStore = vi.mocked(useConnectionStore);

const connectedStatus = {
  connected: true,
  url: 'https://smcp.example.com',
  office_id: 'office-1',
  computer_name: 'my-pc',
  connected_at: '2025-01-01T10:00:00Z',
  profile_name: 'prod',
};

const mockProfiles = [
  { name: 'prod', url: 'https://smcp.example.com', namespace: 'default', office_id: 'office-1', computer_name: 'my-pc', headers: {} },
];

describe('SmcpConnection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockStore.getStatus.mockReturnValue({ connected: false });
    mockUseConnectionStore.mockReturnValue({ ...mockStore } as any);
  });

  it('calls fetchProfiles and fetchStatus on mount', () => {
    render(<SmcpConnection instanceId="computer-a" />);
    expect(mockStore.fetchProfiles).toHaveBeenCalledWith('computer-a');
    expect(mockStore.fetchStatus).toHaveBeenCalledWith('computer-a');
  });

  it('renders disconnected status when not connected', () => {
    render(<SmcpConnection instanceId="computer-a" />);
    expect(screen.getByText('Disconnected')).toBeInTheDocument();
    expect(screen.getByText('Not connected to any SMCP server.')).toBeInTheDocument();
  });

  it('renders connected status with details', () => {
    mockUseConnectionStore.mockReturnValue({
      ...mockStore,
      getStatus: vi.fn(() => connectedStatus),
    } as any);
    render(<SmcpConnection instanceId="computer-a" />);
    expect(screen.getByText('Connected')).toBeInTheDocument();
    expect(screen.getByText('https://smcp.example.com')).toBeInTheDocument();
  });

  it('renders profiles table', () => {
    mockUseConnectionStore.mockReturnValue({ ...mockStore, profiles: mockProfiles } as any);
    render(<SmcpConnection instanceId="computer-a" />);
    expect(screen.getByText('prod')).toBeInTheDocument();
  });

  it('renders error alert', () => {
    mockUseConnectionStore.mockReturnValue({ ...mockStore, error: 'Network error' } as any);
    render(<SmcpConnection instanceId="computer-a" />);
    expect(screen.getByText('Network error')).toBeInTheDocument();
  });

  it('renders add profile button', () => {
    render(<SmcpConnection instanceId="computer-a" />);
    expect(screen.getByText('Add Profile')).toBeInTheDocument();
  });
});
