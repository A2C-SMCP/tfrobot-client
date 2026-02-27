import { Typography, Empty } from 'antd';
import { useTranslation } from 'react-i18next';

const { Paragraph } = Typography;

export function ResourceBrowser() {
  const { t } = useTranslation();

  // Resource listing/reading APIs are not yet exposed on MCPServerManager.
  // This will be implemented when smcp-computer crate adds public resource methods.
  return (
    <Empty description={t('debug.noResources')}>
      <Paragraph type="secondary">{t('debug.resourcesNotAvailable')}</Paragraph>
    </Empty>
  );
}
