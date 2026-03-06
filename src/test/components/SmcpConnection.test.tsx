import { render, screen } from '../helpers/render';
import { vi, describe, it, expect, beforeEach } from 'vitest';
import { SmcpConnection } from '@/components/SmcpConnection';

const mockStore = {
  profiles: [],
  status: { connected: false },
  loading: false,
  error: null,
  fetchProfiles: vi.fn(),
  fetchStatus: vi.fn(),
  saveProfile: vi.fn(),
  deleteProfile: vi.fn(),
  connect: vi.fn(),
  disconnect: vi.fn(),
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
  { name: 'prod', url: 'https://smcp.example.com', namespace: 'default', office_id: 'office-1', computer_name: 'my-pc', headers: {}, auto_connect: false, auto_reconnect: false },
];

describe('SmcpConnection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockUseConnectionStore.mockReturnValue({ ...mockStore } as any);
  });

  it('calls fetchProfiles and fetchStatus on mount', () => {
    render(<SmcpConnection />);
    expect(mockStore.fetchProfiles).toHaveBeenCalled();
    expect(mockStore.fetchStatus).toHaveBeenCalled();
  });

  it('renders disconnected status when not connected', () => {
    render(<SmcpConnection />);
    expect(screen.getByText('Disconnected')).toBeInTheDocument();
    expect(screen.getByText('Not connected to any SMCP server.')).toBeInTheDocument();
  });

  it('renders connected status with details', () => {
    mockUseConnectionStore.mockReturnValue({ ...mockStore, status: connectedStatus } as any);
    render(<SmcpConnection />);
    expect(screen.getByText('Connected')).toBeInTheDocument();
    expect(screen.getByText('https://smcp.example.com')).toBeInTheDocument();
  });

  it('renders profiles table', () => {
    mockUseConnectionStore.mockReturnValue({ ...mockStore, profiles: mockProfiles } as any);
    render(<SmcpConnection />);
    expect(screen.getByText('prod')).toBeInTheDocument();
  });

  it('renders error alert', () => {
    mockUseConnectionStore.mockReturnValue({ ...mockStore, error: 'Network error' } as any);
    render(<SmcpConnection />);
    expect(screen.getByText('Network error')).toBeInTheDocument();
  });

  it('renders add profile button', () => {
    render(<SmcpConnection />);
    expect(screen.getByText('Add Profile')).toBeInTheDocument();
  });
});
