import { managerSessionFromContext, useManagerStore } from '@/stores/managerStore';
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
    context,
    pendingAccountSelection,
    logout,
  } = useManagerStore();
  const session = managerSessionFromContext(context);
  const onboardingUserId = context.authState === 'onboarding_required' ? context.user?.id ?? null : null;

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
          <Button icon={<ArrowLeftOutlined />} onClick={() => void logout()} block>
            {t('managerAccount.onboarding.back')}
          </Button>
        </Space>
      </Card>
    );
  }
  return <LoginForm />;
}
