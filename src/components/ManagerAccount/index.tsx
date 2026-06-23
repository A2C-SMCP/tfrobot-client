import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useManagerStore } from '@/stores/managerStore';
import { LoginForm } from './LoginForm';
import { AccountSelection } from './AccountSelection';
import { EmployeeList } from './EmployeeList';

const AUTH_EXPIRED_EVENT = 'manager:auth-expired';

interface ManagerAccountProps {
  instanceId?: string;
}

export function ManagerAccount({ instanceId }: ManagerAccountProps) {
  const { session, pendingAccountSelection, handleAuthExpired } = useManagerStore();

  useEffect(() => {
    const unlistenPromise = listen<unknown>(AUTH_EXPIRED_EVENT, () => {
      handleAuthExpired();
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten()).catch(() => {
        /* noop */
      });
    };
  }, [handleAuthExpired]);

  if (session) return <EmployeeList instanceId={instanceId} />;
  if (pendingAccountSelection) return <AccountSelection />;
  return <LoginForm />;
}
