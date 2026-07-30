import {
  Button,
  Space,
  Tag,
  Tooltip,
  Typography,
} from 'antd';
import {
  ArrowLeftOutlined,
  DisconnectOutlined,
  LinkOutlined,
  PlayCircleOutlined,
  SettingOutlined,
  StopOutlined,
} from '@ant-design/icons';
import { useTranslation } from 'react-i18next';
import type { ComputerInstance } from '@/stores/computerStore';
import {
  computerStatusColor,
  connectDisabledReasonTranslationKey,
  type ResolvedComputerConnection,
  usesStopAction,
} from './computerActions';
import { ComputerWorkbenchMoreActions } from './ComputerWorkbenchMoreActions';
import styles from './ComputerWorkbench.module.css';

const { Text, Title } = Typography;

interface ComputerWorkbenchHeaderProps {
  instance: ComputerInstance;
  connection: ResolvedComputerConnection;
  loading: boolean;
  onBack: () => void;
  onOpenSettings: () => void;
  onEdit: () => void;
  onDelete: () => Promise<void>;
  onStartStop: () => void;
  onRestart: () => void;
  onConnect: () => void;
  onDisconnect: () => void;
  onOpenLogs: () => void;
}

export function ComputerWorkbenchHeader({
  instance,
  connection,
  loading,
  onBack,
  onOpenSettings,
  onEdit,
  onDelete,
  onStartStop,
  onRestart,
  onConnect,
  onDisconnect,
  onOpenLogs,
}: ComputerWorkbenchHeaderProps) {
  const { t } = useTranslation();
  const stopAction = usesStopAction(instance.status);
  const primaryCapability = stopAction
    ? instance.runtime.actions.stop
    : instance.runtime.actions.start;
  const primaryLabel = instance.status === 'starting'
    ? t('computer.runtime.actionProgress.starting')
    : instance.status === 'stopping'
      ? t('computer.runtime.actionProgress.stopping')
      : stopAction
        ? t('computer.stop')
        : t('computer.start');
  const primaryDisabledReason = primaryCapability.disabled_reason
    ? t(`computer.runtime.actionDisabledReasons.${primaryCapability.disabled_reason}`)
    : undefined;
  const connectDisabledReasonKey = connectDisabledReasonTranslationKey(connection);
  const connectDisabledReason = connectDisabledReasonKey
    ? t(connectDisabledReasonKey)
    : undefined;
  const disconnectDisabledReason = connection.actions.disconnect.disabled_reason
    ? t(`computer.connectionActions.disabledReasons.${connection.actions.disconnect.disabled_reason}`)
    : undefined;
  const connectionDisabledReason = connection.showDisconnect
    ? disconnectDisabledReason
    : connectDisabledReason;

  return (
    <header className={styles.header}>
      <Tooltip title={t('computer.backToList')}>
        <Button
          className={styles.backButton}
          type="text"
          icon={<ArrowLeftOutlined />}
          aria-label={t('computer.backToList')}
          onClick={onBack}
        />
      </Tooltip>

      <div className={styles.identity}>
        <Title level={3} style={{ margin: 0 }}>{instance.name}</Title>
        <div className={styles.identityMeta}>
          <Text type="secondary">{instance.id}</Text>
          {instance.description && <Text>{instance.description}</Text>}
          <Text type="secondary">
            {instance.robotName
              ? t('computer.boundRobot', { name: instance.robotName })
              : t('computer.noRobotBound')}
          </Text>
        </div>
      </div>

      <div className={styles.headerControls}>
        <div className={styles.statuses}>
          <div className={styles.statusItem}>
            <Text type="secondary">{t('computer.runtime.title')}</Text>
            <Tag color={computerStatusColor[instance.status]}>
              {t(`computer.status.${instance.status}`)}
            </Tag>
          </div>
          <div className={styles.statusItem}>
            <Text type="secondary">{t('computer.runtime.connection')}</Text>
            <Tag color={connection.status === 'connected' ? 'green' : 'default'}>
              {t(`computer.connection.${connection.status}`)}
            </Tag>
          </div>
        </div>

        <div className={styles.actions}>
          <Button
            type="primary"
            danger={stopAction}
            icon={stopAction ? <StopOutlined /> : <PlayCircleOutlined />}
            aria-label={primaryLabel}
            disabled={!primaryCapability.enabled}
            loading={loading}
            onClick={onStartStop}
          >
            {primaryLabel}
          </Button>
          {connection.showDisconnect ? (
            <Button
              danger
              icon={<DisconnectOutlined />}
              aria-label={t('connection.disconnect')}
              disabled={!connection.actions.disconnect.enabled}
              loading={connection.isDisconnecting}
              onClick={onDisconnect}
            >
              {t('connection.disconnect')}
            </Button>
          ) : (
            <Button
              icon={<LinkOutlined />}
              aria-label={t('connection.connect')}
              disabled={!connection.canConnect}
              loading={connection.isConnecting}
              onClick={onConnect}
            >
              {t('connection.connect')}
            </Button>
          )}
          <Tooltip title={t('computer.settings.open')}>
            <Button
              icon={<SettingOutlined />}
              aria-label={t('computer.settings.open')}
              onClick={onOpenSettings}
            />
          </Tooltip>
          <ComputerWorkbenchMoreActions
            instance={instance}
            loading={loading}
            onRestart={onRestart}
            onOpenLogs={onOpenLogs}
            onEdit={onEdit}
            onDelete={onDelete}
          />
        </div>

        {(primaryDisabledReason || connectionDisabledReason) && (
          <Space direction="vertical" size={0} className={styles.actionGuidance}>
            {primaryDisabledReason && <Text type="secondary">{primaryDisabledReason}</Text>}
            {connectionDisabledReason && <Text type="secondary">{connectionDisabledReason}</Text>}
          </Space>
        )}
      </div>
    </header>
  );
}
