import { Badge } from 'antd';
import { useTranslation } from 'react-i18next';

interface ServerStatusBadgeProps {
  running: boolean;
  statusMessage?: string;
}

export function ServerStatusBadge({ running, statusMessage }: ServerStatusBadgeProps) {
  const { t } = useTranslation();

  if (running) {
    return (
      <Badge
        status="success"
        text={t('mcp.status.running')}
      />
    );
  }

  // Check if it's an error state
  if (statusMessage && statusMessage.toLowerCase().includes('error')) {
    return (
      <Badge
        status="error"
        text={t('mcp.status.error')}
      />
    );
  }

  return (
    <Badge
      status="default"
      text={t('mcp.status.stopped')}
    />
  );
}
