import { invoke } from '@tauri-apps/api/core';
import { createRoot } from 'react-dom/client';
import { ConfigProvider } from 'antd';
import { Chat } from '../../src/components/Chat';
import { useManagerStore, managerContextScope } from '../../src/stores/managerStore';
import i18n from '../../src/i18n';
void i18n.changeLanguage('en');

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
  employees: [42, 43].map((id) => ({ id, name: `Robot ${id}`, robotAccountId: `fixture-${id}`, status: 'running', templateType: 'tfrserver' })),
  loading: false, error: null, paymentRequired: null, lastFetchAt: Date.now(),
  selectedEmployeeId: null, connectingEmployeeId: null,
} } });
const seen = new Set<string>();
new MutationObserver(() => {
  const markers = document.body.innerText.match(/History (?:42|99)/g) ?? [];
  for (const marker of markers) {
    if (!seen.has(marker)) { seen.add(marker); void invoke('acceptance_event', { event: { kind: 'render', marker } }); }
  }
}).observe(document.getElementById('root')!, { childList: true, subtree: true, characterData: true });
window.addEventListener('error', (event) => { void invoke('acceptance_event', { event: { kind: 'error', message: event.message } }); });
createRoot(document.getElementById('root')!).render(<ConfigProvider><Chat /></ConfigProvider>);
