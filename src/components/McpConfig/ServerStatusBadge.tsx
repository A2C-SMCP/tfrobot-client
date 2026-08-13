import { Badge } from 'antd';
import { useTranslation } from 'react-i18next';
import type { McpServerStatus } from '@/stores/mcpStore';

type ServerDisplayStatus =
  | 'stopped'
  | 'disconnected'
  | 'connecting'
  | 'connected'
  | 'authorization_required'
  | 'error';

function projectServerDisplayStatus(
  server: Pick<McpServerStatus, 'activation_state' | 'connection_state'>,
): ServerDisplayStatus {
  if (server.activation_state === 'stopped') return 'stopped';
  return server.connection_state;
}

interface ServerStatusBadgeProps {
  server: Pick<McpServerStatus, 'activation_state' | 'connection_state'>;
}

export function ServerStatusBadge({ server }: ServerStatusBadgeProps) {
  const { t } = useTranslation();
  const status = projectServerDisplayStatus(server);

  switch (status) {
    case 'connected':
      return <Badge status="success" text={t('mcp.status.connected')} />;
    case 'connecting':
      return <Badge status="processing" text={t('mcp.status.connecting')} />;
    case 'authorization_required':
      return <Badge status="warning" text={t('mcp.status.authorizationRequired')} />;
    case 'error':
      return <Badge status="error" text={t('mcp.status.error')} />;
    case 'disconnected':
      return <Badge status="warning" text={t('mcp.status.disconnected')} />;
    default:
      return <Badge status="default" text={t('mcp.status.stopped')} />;
  }
}
