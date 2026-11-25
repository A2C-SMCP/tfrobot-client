import { Button, Space, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import type { ConfigSaveState } from './useConfigCommit';

export function ConfigSaveStatus({ state, onRetry }: { state: ConfigSaveState; onRetry?: () => unknown }) {
  const { t } = useTranslation();
  return <Space role="status" aria-live="polite">
    <Typography.Text type={state === 'error' ? 'danger' : 'secondary'}>{t(`computer.configSave.${state}`)}</Typography.Text>
    {state === 'error' && onRetry && <Button size="small" onClick={() => void onRetry()}>{t('computer.configSave.retry')}</Button>}
  </Space>;
}
