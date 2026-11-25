import { Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import type { ComputerInstance } from '@/stores/computerStore';
import styles from './ComputerWorkbench.module.css';

export function ComputerIdentity({ instance, settings = false }: {
  instance: Pick<ComputerInstance, 'id' | 'name' | 'description'>;
  settings?: boolean;
}) {
  const { t } = useTranslation();
  return <div className={styles.identity}>
    <Typography.Title level={3} style={{ margin: 0 }}>
      {settings ? t('computer.settings.title', { name: instance.name }) : instance.name}
    </Typography.Title>
    <div className={styles.identityMeta}>
      {instance.description && <Typography.Text type="secondary">{instance.description}</Typography.Text>}
      <Typography.Text type="secondary" title={instance.id} copyable={{ text: instance.id, tooltips: [t('computer.workbench.actions.copyId'), t('computer.workbench.messages.idCopied')] }}>
        {t('computer.identity.identifier')}
      </Typography.Text>
    </div>
  </div>;
}
