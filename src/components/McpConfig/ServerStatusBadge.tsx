import { Badge } from 'antd';
import { useTranslation } from 'react-i18next';

export type ServerDisplayStatus = 'running' | 'error' | 'pending' | 'stopped';

interface ServerStatusBadgeProps {
  status: ServerDisplayStatus;
}

export function ServerStatusBadge({ status }: ServerStatusBadgeProps) {
  const { t } = useTranslation();

  switch (status) {
    case 'running':
      return <Badge status="success" text={t('mcp.status.running')} />;
    case 'error':
      return <Badge status="error" text={t('mcp.status.error')} />;
    case 'pending':
      return <Badge status="processing" text={t('mcp.status.pending')} />;
    default:
      return <Badge status="default" text={t('mcp.status.stopped')} />;
  }
}
