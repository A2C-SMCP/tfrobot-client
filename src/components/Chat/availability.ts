import type { DigitalEmployeeBrief } from '@/stores/managerStore';

export type ChatRobotDisabledReason = 'notRunning' | 'missingAccount' | 'incompatible';

export function chatRobotDisabledReason(
  employee: DigitalEmployeeBrief,
): ChatRobotDisabledReason | null {
  if ((employee.status ?? 'running') !== 'running') return 'notRunning';
  if (!employee.robotAccountId) return 'missingAccount';
  if (employee.templateType && employee.templateType !== 'tfrserver') return 'incompatible';
  return null;
}
