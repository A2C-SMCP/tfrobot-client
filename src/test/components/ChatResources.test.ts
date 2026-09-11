import { createElement } from 'react';
import { ChatResourceProvider, ChatResourceView } from '@turingfocus/chat-kit';
import { render, screen, fireEvent, waitFor } from '../helpers/render';
import { chatUiLabels } from '@/components/Chat/chatBridge';
import i18n from '@/i18n';
import { invoke } from '@tauri-apps/api/core';
import { describe, beforeEach, expect, it, vi } from 'vitest';
import type { ChatResourceRequest } from '@turingfocus/chat-kit/headless';
import { createChatResourcePort } from '@/components/Chat/chatResources';

function request(controller = new AbortController(), purpose: ChatResourceRequest['purpose'] = 'display'): ChatResourceRequest {
  return {
    resource: { uri: 's3://bucket/photo.png', name: '照片.png' }, purpose,
    signal: {
      get aborted() { return controller.signal.aborted; },
      subscribe(callback) {
        controller.signal.addEventListener('abort', callback, { once: true });
        return () => controller.signal.removeEventListener('abort', callback);
      },
    },
  };
}
const handle = { id: 'opaque', url: 'http://127.0.0.1:12345/resource/opaque' };

describe('native private resource lifecycle', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(invoke).mockImplementation(async (command) => command === 'chat_resolve_resource' ? handle : undefined);
  });

  it('releases a late registration after cancellation and never exposes its URL', async () => {
    let complete!: (value: typeof handle) => void;
    vi.mocked(invoke).mockImplementation((command) => command === 'chat_resolve_resource'
      ? new Promise((resolve) => { complete = resolve; }) : Promise.resolve());
    const controller = new AbortController();
    const pending = createChatResourcePort('lease').resolve!(request(controller));
    controller.abort();
    complete(handle);
    await expect(pending).rejects.toMatchObject({ code: 'cancelled' });
    expect(invoke).toHaveBeenCalledWith('chat_release_resource', { leaseId: 'lease', resourceId: 'opaque' });
  });

  it('releases exactly once when both signal and Kit dispose the same resource', async () => {
    const controller = new AbortController();
    const resolved = await createChatResourcePort('lease').resolve!(request(controller));
    controller.abort();
    resolved.dispose?.();
    expect(vi.mocked(invoke).mock.calls.filter(([name]) => name === 'chat_release_resource')).toHaveLength(1);
  });

  it.each(['open', 'download'] as const)('uses the native %s command and releases the operation handle', async (purpose) => {
    await createChatResourcePort('lease')[purpose]!(request(undefined, purpose));
    expect(invoke).toHaveBeenCalledWith('chat_save_resource', {
      leaseId: 'lease', resourceId: 'opaque', name: '照片.png', open: purpose === 'open',
    });
    expect(invoke).toHaveBeenCalledWith('chat_release_resource', { leaseId: 'lease', resourceId: 'opaque' });
    expect(vi.mocked(invoke).mock.calls.some(([name]) => name === 'chat_prepare_transfer')).toBe(false);
  });

  it('revokes an in-flight native download when its UI scope is cancelled', async () => {
    let started!: () => void;
    const running = new Promise<void>((resolve) => { started = resolve; });
    let fail!: (error: unknown) => void;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'chat_resolve_resource') return handle;
      if (command === 'chat_save_resource') { started(); return new Promise((_, reject) => { fail = reject; }); }
    });
    const controller = new AbortController();
    const onError = vi.fn();
    const pending = createChatResourcePort('lease', onError).download!(request(controller, 'download'));
    await running;
    controller.abort();
    expect(invoke).toHaveBeenCalledWith('chat_release_resource', { leaseId: 'lease', resourceId: 'opaque' });
    fail({ code: 'cancelled' });
    await expect(pending).rejects.toMatchObject({ code: 'cancelled' });
    expect(onError).not.toHaveBeenCalled();
  });

  it('reports safe error codes and releases failed saves', async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'chat_resolve_resource') return handle;
      if (command === 'chat_save_resource') throw { code: 'permission' };
    });
    const onError = vi.fn();
    await expect(createChatResourcePort('lease', onError).download!(request(undefined, 'download'))).rejects.toMatchObject({ code: 'unauthorized' });
    expect(onError).toHaveBeenCalledWith('permission');
    expect(invoke).toHaveBeenCalledWith('chat_release_resource', { leaseId: 'lease', resourceId: 'opaque' });
  });

  it.each(['https://internal.example/file', 'http://127.0.0.1:12345/resource/wrong', 'http://127.0.0.1:12345/resource/opaque?token=bad'])(
    'rejects non-capability resolver response %s', async (url) => {
      vi.mocked(invoke).mockImplementation(async (command) => command === 'chat_resolve_resource' ? { ...handle, url } : undefined);
      await expect(createChatResourcePort('lease').resolve!(request())).rejects.toThrow();
      expect(invoke).toHaveBeenCalledWith('chat_release_resource', { leaseId: 'lease', resourceId: 'opaque' });
    },
  );
});

// Exercise the published resource renderer, including its retry policy and action UI.


function renderResource(kind: 'image' | 'file' = 'image', onError = vi.fn()) {
  const port = createChatResourcePort('lease', onError);
  return render(createElement(ChatResourceProvider, { port, scope: 'lease' },
    createElement(ChatResourceView, {
      resource: { uri: 's3://bucket/photo.png', name: 'photo.png' }, kind,
      labels: chatUiLabels((key) => i18n.t(key)),
    })));
}

describe('0.8.1 resource errors and translations through the published UI', () => {
  beforeEach(async () => { vi.clearAllMocks(); await i18n.changeLanguage('en'); });

  it.each([
    ['permission', 'unauthorized', true], ['not_found', 'not-found', false],
    ['timeout', 'network', true], ['busy', 'network', true],
    ['unsupported', 'unsupported', false], ['cancelled', 'cancelled', false],
    ['save', 'unknown', true], ['too_large', 'unknown', true], ['invalid', 'unknown', true],
    ['untrusted-message-token', 'unknown', true],
  ] as const)('maps %s and applies the Kit retry policy', async (native, kit, retryable) => {
    vi.mocked(invoke).mockRejectedValue({ code: native, message: 'secret payload must not reach UI' });
    renderResource();
    const labels = chatUiLabels((key) => i18n.t(key));
    expect(await screen.findByText(labels.resource![kit]!)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Retry resource' }) !== null).toBe(retryable);
    expect(screen.queryByText(/secret payload/)).not.toBeInTheDocument();
    if (kit === 'cancelled') expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    if (retryable) {
      vi.mocked(invoke).mockResolvedValue(handle);
      fireEvent.click(screen.getByRole('button', { name: 'Retry resource' }));
      await waitFor(() => expect(screen.getByRole('img', { name: 'photo.png' })).toHaveAttribute('src', handle.url));
    }
  });

  it.each(['en', 'zh'])('localizes native download failure and cancellation in %s', async (language) => {
    await i18n.changeLanguage(language);
    let code = 'permission';
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'chat_resolve_resource') return handle;
      if (command === 'chat_save_resource') throw { code };
    });
    const onError = vi.fn(); renderResource('file', onError);
    const labels = chatUiLabels((key) => i18n.t(key)).resource!;
    fireEvent.click(screen.getByRole('button', { name: labels.download! }));
    expect(await screen.findByText(labels.unauthorized!)).toBeInTheDocument();
    expect(onError).toHaveBeenCalledWith('permission');
    code = 'cancelled'; onError.mockClear();
    fireEvent.click(screen.getByRole('button', { name: labels.download! }));
    await waitFor(() => expect(screen.getByRole('button', { name: labels.download! })).toBeEnabled());
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(onError).not.toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledWith('chat_release_resource', { leaseId: 'lease', resourceId: handle.id });
  });
});
