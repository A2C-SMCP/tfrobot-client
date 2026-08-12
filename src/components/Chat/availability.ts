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

export function orderChatRobots(
  employees: DigitalEmployeeBrief[],
  locale: string,
): DigitalEmployeeBrief[] {
  const collator = new Intl.Collator(locale, { sensitivity: 'base', numeric: true });
  return [...employees].sort((left, right) => (
    collator.compare(left.name, right.name) || left.id - right.id
  ));
}

export function preferredChatRobotId(
  orderedEmployees: DigitalEmployeeBrief[],
  selectedEmployeeId: number | null,
  recentEmployeeId: number | null,
): number | null {
  const available = orderedEmployees.filter(
    (employee) => chatRobotDisabledReason(employee) === null,
  );
  if (available.some((employee) => employee.id === selectedEmployeeId)) {
    return selectedEmployeeId;
  }
  return available.find((employee) => employee.id === recentEmployeeId)?.id
    ?? available[0]?.id
    ?? null;
}
