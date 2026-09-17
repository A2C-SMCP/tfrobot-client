import { sanitizeDiagnosticText } from '@turingfocus/chat-kit/headless';
import { createClientChatFactory, getChatDeadlineAt } from '../../src/components/Chat/chatBridge';

// Test-only entry, bundled exclusively by this acceptance harness.
export async function verifyUpgrade(original: string) {
  for (let index = 0; index < 3000; index++) sanitizeDiagnosticText('ordinary warmup');
  const diagnostics: unknown[] = [];
  const client = createClientChatFactory({
    descriptor: { leaseId: 'upgrade-probe', employeeId: 42, robotName: 'Probe',
      httpBaseUrl: 'http://127.0.0.1:18767/', socketNamespaceUrl: 'http://127.0.0.1:18767/chat', socketPath: '/socket.io' },
    messageCreator: { uid: 'test', name: 'Test' },
    onDiagnostic: error => diagnostics.push(error), onUnhandledError: error => diagnostics.push(error),
  }).create();
  const timings = [];
  try {
    for (const conversationId of ['90', '91', '90']) {
      const start = performance.now();
      const loaded = await client.loadConversation({ conversationId, deadlineAt: getChatDeadlineAt() });
      const elapsedMs = performance.now() - start;
      if (!loaded.ok || elapsedMs >= 15_000) throw new Error('Long history failed or exceeded host deadline');
      const snapshot = client.getSnapshot();
      const item = snapshot?.timeline.find(item => item.id === `long-tool-${conversationId}`);
      const value = item?.kind === 'agent-event' && item.eventCategory === 'tool'
        ? item.transitions.slice(-1)[0]?.toolReturn?.result : undefined;
      const expected = original.replace('token=embedded-secret', 'token=[REDACTED]').split('fixture').join('[REDACTED]');
      if (value !== expected) throw new Error('Shell output changed or credentials were not removed');
      const serialized = JSON.stringify(snapshot);
      if (serialized.includes('embedded-secret') || serialized.includes('fixture')) throw new Error('Credential leaked in snapshot');
      timings.push({ conversationId, elapsedMs, length: expected.length });
    }
    if (diagnostics.length) throw new Error('Unexpected diagnostics during long history');
    return { timings, redacted: true, preserved: true };
  } finally {
    await client.dispose({ deadlineAt: getChatDeadlineAt() });
  }
}
