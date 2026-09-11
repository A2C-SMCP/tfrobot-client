import { createRoot } from 'react-dom/client';
import { ConfigProvider } from 'antd';
import { Chat } from '../../src/components/Chat';
import { useManagerStore, managerContextScope } from '../../src/stores/managerStore';
import '../../src/i18n';

// Only identity/session acquisition is synthetic. Chat, its factory, Socket.IO,
// history recovery and the frontend Tauri HTTP bridge are production modules.
const context = {
  revision: 1, authState: 'authenticated' as const, environment: 'staging' as const,
  contextKey: { environment: 'staging' as const, accountId: 'fixture', organizationId: 'fixture' },
  user: { id: 'fixture', nickname: 'Fixture', email: '', phone: '' },
  account: { id: 'fixture', name: 'Fixture', nickname: 'Fixture', avatar: '', employeeNo: '' },
  organization: { id: 'fixture', name: 'Local background test', organizationType: 'company' },
  permissions: [],
};
const scope = managerContextScope(context)!;
useManagerStore.setState({ context, employeeResources: { [scope]: {
  scope, contextKey: context.contextKey, revision: 1,
  employees: [{ id: 42, name: 'Fixture Robot', robotAccountId: 'fixture', status: 'running', templateType: 'tfrserver' }],
  loading: false, error: null, paymentRequired: null, lastFetchAt: Date.now(),
  selectedEmployeeId: null, connectingEmployeeId: null,
} } });
const observations: unknown[] = [];
const record = (kind: string, data: unknown) => observations.push({ kind, data, at: Date.now() });
document.addEventListener('visibilitychange', () => record('visibility', document.visibilityState));
window.addEventListener('error', event => record('error', event.message));
window.addEventListener('unhandledrejection', event => record('rejection', String(event.reason)));
async function report(name: string) {
  await fetch('http://127.0.0.1:18766/snapshot', { method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name, text: document.body.innerText, observedAt: Date.now(),
      visibility: document.visibilityState, observations }) });
}
Object.assign(window, { backgroundSnapshot: report });
// Observe actual React commits. An event is sent only after a target text has
// rendered; the controller must receive it BEFORE it issues any window eval.
const rendered = new Set<string>();
new MutationObserver(() => {
  const markers = document.body.innerText.match(/INITIAL-HISTORY|MISSED-WHILE-OFFLINE|LIVE-[A-Za-z0-9-]+/g) ?? [];
  for (const marker of markers) {
    if (rendered.has(marker)) continue;
    rendered.add(marker);
    void report(`render:${marker}`).catch(error => record('report-error', String(error)));
  }
}).observe(document.getElementById('root')!, { childList: true, subtree: true, characterData: true });
createRoot(document.getElementById('root')!).render(<ConfigProvider><Chat /></ConfigProvider>);
