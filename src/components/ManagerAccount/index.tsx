import { useManagerStore } from '@/stores/managerStore';
import { LoginForm } from './LoginForm';
import { AccountSelection } from './AccountSelection';
import { EmployeeList } from './EmployeeList';

interface ManagerAccountProps {
  instanceId?: string;
}

export function ManagerAccount({ instanceId }: ManagerAccountProps) {
  const {
    session,
    pendingAccountSelection,
  } = useManagerStore();

  if (session) return <EmployeeList instanceId={instanceId} />;
  if (pendingAccountSelection) return <AccountSelection />;
  return <LoginForm />;
}
