import { managerSessionFromContext, useManagerStore } from '@/stores/managerStore';
import { LoginForm } from './LoginForm';
import { AccountSelection } from './AccountSelection';
import { EmployeeList } from './EmployeeList';
import { OnboardingNotice } from './OnboardingNotice';
import { Card, Space } from 'antd';

interface ManagerAccountProps {
  instanceId?: string;
}

export function ManagerAccount({ instanceId }: ManagerAccountProps) {
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
          <OnboardingNotice withBackIcon onBack={() => void logout()} />
        </Space>
      </Card>
    );
  }
  return <LoginForm />;
}
