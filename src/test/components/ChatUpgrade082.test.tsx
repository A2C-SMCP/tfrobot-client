import { ChatComposer } from '@turingfocus/chat-kit';
import { describe, expect, it, vi } from 'vitest';
import { chatUiLabels } from '@/components/Chat/chatBridge';
import i18n from '@/i18n';
import { fireEvent, render, screen, waitFor } from '../helpers/render';

describe('0.8.2 published composer with the host Enter policy', () => {
  it('guards IME, modifiers, held keys and pending sends, and retains attachments', async () => {
    let complete!: (sent: boolean) => void;
    const onSend = vi.fn(() => new Promise<boolean>((resolve) => { complete = resolve; }));
    const attachment = { uri: 's3://fixture/a.png', mimeType: 'image/png', name: 'a.png' };
    render(<ChatComposer sendShortcut="enter" onSend={onSend}
      labels={chatUiLabels((key) => i18n.t(key))}
      draft={{ conversationId: '42', revision: 1, text: '你好', attachments: [attachment], longTexts: [] }} />);
    const input = screen.getByRole('textbox', { name: 'Message' });
    for (const options of [{ shiftKey: true }, { metaKey: true }, { altKey: true }, { repeat: true }, { isComposing: true }, { keyCode: 229 }]) {
      fireEvent.keyDown(input, { key: 'Enter', ...options });
    }
    fireEvent.compositionStart(input);
    fireEvent.keyDown(input, { key: 'Enter' });
    fireEvent.compositionEnd(input);
    fireEvent.keyDown(input, { key: 'Enter', keyCode: 229 });
    expect(onSend).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(onSend).toHaveBeenCalledTimes(1));
    fireEvent.keyDown(input, { key: 'Enter', ctrlKey: true });
    expect(onSend).toHaveBeenCalledTimes(1);
    expect(onSend.mock.calls[0]).toEqual(['你好', [attachment]]);
    complete(false);
    await waitFor(() => expect(screen.getByRole('button', { name: /Send/ })).toBeEnabled());
    fireEvent.keyDown(input, { key: 'Enter', ctrlKey: true });
    await waitFor(() => expect(onSend).toHaveBeenCalledTimes(2));
    complete(true);
  }, 15_000);

  it.each(['en', 'zh'])('provides the new shortcut, diagnostic and recovery copy in %s', (language) => {
    const labels = chatUiLabels((key) => i18n.t(key, { lng: language }));
    for (const state of ['connecting', 'joining', 'active', 'degraded', 'reconnecting', 'recovering', 'auth-required', 'offline', 'subscription-failed'] as const) {
      expect(labels.lifecycleStatus?.[state]).toBeTruthy();
      if (language === 'zh') expect(labels.lifecycleStatus?.[state]).toMatch(/[\u4e00-\u9fff]/);
    }
    for (const key of ['composerEnterHint', 'composerCtrlEnterHint', 'diagnostics', 'copyDiagnostic', 'diagnosticCopied', 'diagnosticCopyFailed', 'diagnosticDetails', 'noDiagnostics', 'activeFaults', 'signInAgain', 'recoveredComplete', 'recoveredBestEffort'] as const) {
      expect(labels[key]).toBeTruthy();
      expect(labels[key]).not.toContain('chat.workspace.');
      if (language === 'zh') expect(labels[key]).toMatch(/[\u4e00-\u9fff]/);
    }
  });

  it('keeps unknown send outcomes actionable without reflecting raw error fields', () => {
    const labels = chatUiLabels((key) => i18n.t(key, { lng: 'zh' }));
    expect(labels.formatChatError?.({ code: 'timeout', retryable: false,
      message: 'token=private-message', details: { password: 'private-details' },
      diagnostic: { operation: 'sendText', outcome: 'unknown', phase: 'private-phase' },
    })).toBe('发送消息: 操作超时。 结果未知，请先查看会话历史，再决定是否重新发送。');
    expect(labels.formatChatError?.({ code: 'unknown', retryable: false,
      message: 'secret', diagnostic: { operation: 'private-operation' },
    })).toBe('聊天操作: 未提供错误原因。 操作未完成。');
  });
});
