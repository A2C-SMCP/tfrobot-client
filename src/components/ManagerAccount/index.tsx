import { useManagerStore } from '@/stores/managerStore';
import { LoginForm } from './LoginForm';
import { AccountSelection } from './AccountSelection';
import { EmployeeList } from './EmployeeList';
import { Alert, Button, Card, Space } from 'antd';
import { ArrowLeftOutlined } from '@ant-design/icons';
import { useTranslation } from 'react-i18next';

interface ManagerAccountProps {
  instanceId?: string;
}

export function ManagerAccount({ instanceId }: ManagerAccountProps) {
  const { t } = useTranslation();
  const {
    session,
    pendingAccountSelection,
    onboardingUserId,
    reset,
  } = useManagerStore();

  if (session) return <EmployeeList instanceId={instanceId} />;
  if (pendingAccountSelection) return <AccountSelection />;
  if (onboardingUserId !== null) {
    return (
      <Card>
        <Space direction="vertical" size="large" style={{ width: '100%' }}>
          <Alert
            type="info"
            showIcon
            message={t('managerAccount.onboarding.title')}
            description={t('managerAccount.onboarding.description')}
          />
          <Button icon={<ArrowLeftOutlined />} onClick={reset} block>
            {t('managerAccount.onboarding.back')}
          </Button>
        </Space>
      </Card>
    );
  }
  return <LoginForm />;
}
