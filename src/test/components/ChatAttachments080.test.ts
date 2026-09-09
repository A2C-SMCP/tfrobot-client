import { invoke } from '@tauri-apps/api/core';
import { createElement, useState } from 'react';
import { ChatComposer, ChatMarkdownContent, ChatResourceProvider, type ChatAttachmentUploader, type ComposerDraft } from '@turingfocus/chat-kit';
import { render, fireEvent, screen, waitFor } from '../helpers/render';
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { createTFRobotChatGateway } from '@turingfocus/chat-kit/headless';
import { createClientChatFactory, createTauriChatFetch, chatUiLabels } from '@/components/Chat/chatBridge';
import { createChatResourcePort } from '@/components/Chat/chatResources';
import { invokeChatTransfer, MAX_CHAT_FILE_BYTES } from '@/components/Chat/chatTransport';

// JSDOM's File has FileReader support but lacks the browser Blob.arrayBuffer method.
const originalArrayBuffer = Object.getOwnPropertyDescriptor(Blob.prototype, 'arrayBuffer');
beforeAll(() => Object.defineProperty(Blob.prototype, 'arrayBuffer', {
  configurable: true,
  value(this: Blob) {
    return new Promise<ArrayBuffer>((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(reader.result as ArrayBuffer);
      reader.onerror = () => reject(reader.error);
      reader.readAsArrayBuffer(this);
    });
  },
}));
afterAll(() => {
  if (originalArrayBuffer) Object.defineProperty(Blob.prototype, 'arrayBuffer', originalArrayBuffer);
  else Reflect.deleteProperty(Blob.prototype, 'arrayBuffer');
});

const descriptor = {
  leaseId: 'lease-080', employeeId: 42, robotName: 'Robot',
  httpBaseUrl: 'https://robot.example/proxy/', socketNamespaceUrl: 'https://robot.example/chat', socketPath: '/socket.io',
};
const response = (data: unknown, status = 200) => ({
  status, contentType: 'application/json', body: JSON.stringify({ code: status, message: 'test', data }),
});
const deadlineAt = () => Date.now() + 10_000;
const factory = () => createClientChatFactory({
  descriptor, messageCreator: { uid: 'user', name: 'User' }, onDiagnostic: vi.fn(), onUnhandledError: vi.fn(),
});
const signal = () => ({ aborted: false, subscribe: () => () => undefined });

function ComposerHarness({ uploader, onSend }: {
  uploader: ChatAttachmentUploader;
  onSend: (text: string, attachments?: readonly unknown[]) => boolean;
}) {
  const [draft, setDraft] = useState<ComposerDraft>({
    conversationId: '42', revision: 0, text: '', attachments: [], longTexts: [],
  });
  return createElement(ChatComposer, {
    draft, attachmentUploader: uploader, getDeadlineAt: deadlineAt, onSend,
    onDraftChange: (next) => setDraft((current) => ({ ...current, ...next, revision: current.revision + 1 })),
  });
}

describe('Chat Kit 0.8.0 published attachment and host resource contracts', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'chat_get_session_token') return { token: 'short-token', expiresAt: deadlineAt() };
      if (command === 'chat_prepare_transfer') return 'transfer-1';
      if (command === 'chat_release_resource') return undefined;
      if (command === 'chat_cancel_transfer') return undefined;
      if (command === 'chat_upload_request') return response({ uri: 's3://bucket/image.png' });
      if (command === 'chat_resolve_resource') return { id: 'resource-1', url: 'http://127.0.0.1:43210/resource/resource-1' };
      if (command === 'chat_http_request') return response({ task_id: 'real-run' });
      throw new Error(`Unexpected command ${command}`);
    });
  });

  it('uses the published default uploader and sends binary image bytes through typed IPC', async () => {
    const uploader = factory().createAttachmentUploader!();
    try {
      const result = await uploader.upload({
        blob: new Blob([new Uint8Array([0, 255, 128, 13, 10, 1])], { type: 'image/png' }),
        fileName: '图.png', mimeType: 'image/png', deadlineAt: deadlineAt(),
      });
      expect(result).toEqual({ ok: true, value: { uri: 's3://bucket/image.png', mimeType: 'image/png', name: '图.png', size: 6 } });
      expect(invoke).toHaveBeenCalledWith('chat_upload_request', {
        leaseId: 'lease-080', transferId: 'transfer-1',
        url: 'https://robot.example/proxy/v1/dashboard/remote/source/cos/upload',
        file: { name: '图.png', mimeType: 'image/png', dataBase64: 'AP+ADQoB' },
      });
      expect(vi.mocked(invoke).mock.calls.some(([name]) => name === 'chat_http_request')).toBe(false);
    } finally { await uploader.dispose?.(); }
  });

  it('lets the real Gateway turn an uploaded image into the server message format', async () => {
    const gateway = createTFRobotChatGateway({
      baseUrl: descriptor.httpBaseUrl, serverProfile: { kind: 'current-server' },
      sessionProvider: { getSession: () => ({ kind: 'bearer', token: 'short-token' }) },
      messageCreatorProvider: () => ({ uid: 'user', name: 'User' }), fetch: createTauriChatFetch('lease-080'),
    });
    try {
      const result = await gateway.sendMessage!({
        conversationId: '42', deadlineAt: deadlineAt(), text: 'Look at this',
        attachments: [{ uri: 's3://bucket/image.png', mimeType: 'image/png', name: '图.png', size: 6 }],
      });
      expect(result.ok).toBe(true);
      const call = vi.mocked(invoke).mock.calls.find(([name]) => name === 'chat_http_request');
      const body = JSON.parse((call?.[1] as { body: string }).body);
      expect(JSON.stringify(body)).toContain('s3://bucket/image.png');
      expect(JSON.stringify(body)).toContain('Look at this');
      expect(JSON.stringify(body)).not.toContain('dataBase64');
    } finally { await gateway.dispose({ deadlineAt: deadlineAt() }); }
  });

  it('bounds file encoding before IPC and cancels queued uploads without reading their bytes', async () => {
    const reads = vi.spyOn(Blob.prototype, 'arrayBuffer');
    const completions: Array<(value: ReturnType<typeof response>) => void> = [];
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'chat_prepare_transfer') return `transfer-${completions.length}`;
      if (command === 'chat_upload_request') return new Promise((resolve) => { completions.push(resolve); });
    });
    const fetchA = createTauriChatFetch('encoding-lease');
    const fetchB = createTauriChatFetch('encoding-lease');
    const upload = (fetch: typeof globalThis.fetch, signal?: AbortSignal) => {
      const form = new FormData();
      form.append('file', new File(['image'], 'photo.png', { type: 'image/png' }));
      return fetch('https://robot.example/upload', { method: 'POST', body: form, signal });
    };
    const first = upload(fetchA);
    const second = upload(fetchB);
    await waitFor(() => expect(completions).toHaveLength(2));
    const count = reads.mock.calls.length;
    const controller = new AbortController();
    const third = upload(fetchA, controller.signal);
    controller.abort();
    await expect(third).rejects.toMatchObject({ name: 'AbortError' });
    expect(reads).toHaveBeenCalledTimes(count);
    for (const complete of completions) complete(response({ uri: 's3://bucket/photo.png' }));
    await Promise.all([first, second]);
    reads.mockRestore();
  });

  it('rejects oversized files before reserving or sending bytes', async () => {
    const uploader = factory().createAttachmentUploader!();
    try {
      const result = await uploader.upload({
        blob: new Blob([new Uint8Array(MAX_CHAT_FILE_BYTES + 1)]), fileName: 'large.bin', deadlineAt: deadlineAt(),
      });
      expect(result.ok).toBe(false);
      expect(vi.mocked(invoke).mock.calls.some(([name]) => name === 'chat_prepare_transfer')).toBe(false);
    } finally { await uploader.dispose?.(); }
  });

  it('preserves permission failures and permits a later text request', async () => {
    const defaultInvoke = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (name, args) => name === 'chat_upload_request'
      ? response({}, 403) : defaultInvoke(name, args));
    const uploader = factory().createAttachmentUploader!();
    try {
      const result = await uploader.upload({ blob: new Blob(['file']), fileName: 'file.txt', deadlineAt: deadlineAt() });
      expect(result).toMatchObject({ ok: false, error: { code: 'authorization' } });
      const text = await createTauriChatFetch('lease-080')('https://robot.example/proxy/v1/chat/conversations/42/messages', {
        method: 'POST', body: JSON.stringify({ text: 'still works' }),
      });
      expect(text.status).toBe(200);
    } finally { await uploader.dispose?.(); }
  });

  it('cancels a reservation when abort arrives while registration is pending', async () => {
    let finish!: (id: string) => void;
    vi.mocked(invoke).mockImplementation(async (name) => name === 'chat_prepare_transfer'
      ? new Promise<string>((resolve) => { finish = resolve; }) : undefined);
    const controller = new AbortController();
    const pending = invokeChatTransfer('lease-080', 'chat_upload_request', {}, controller.signal);
    controller.abort();
    finish('late-slot');
    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });
    expect(invoke).toHaveBeenCalledWith('chat_cancel_transfer', { leaseId: 'lease-080', transferId: 'late-slot' });
    expect(vi.mocked(invoke).mock.calls.some(([name]) => name === 'chat_upload_request')).toBe(false);
  });

  it('propagates inflight cancellation and consumes a late IPC rejection', async () => {
    let rejectUpload!: (reason: Error) => void;
    vi.mocked(invoke).mockImplementation(async (name) => {
      if (name === 'chat_prepare_transfer') return 'active-slot';
      if (name === 'chat_upload_request') return new Promise((_, reject) => { rejectUpload = reject; });
      return undefined;
    });
    const controller = new AbortController();
    const pending = invokeChatTransfer('lease-080', 'chat_upload_request', {}, controller.signal);
    await vi.waitFor(() => expect(rejectUpload).toBeDefined());
    controller.abort();
    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });
    rejectUpload(new Error('Rust cancelled'));
    expect(invoke).toHaveBeenCalledWith('chat_cancel_transfer', { leaseId: 'lease-080', transferId: 'active-slot' });
  });

  it('resolves private resources under the lease and leaves public URLs direct', async () => {
    const port = createChatResourcePort('lease-080');
    expect(await port.resolve!({ resource: { uri: 's3://bucket/image.png' }, purpose: 'display', signal: signal() }))
      .toEqual({ url: 'http://127.0.0.1:43210/resource/resource-1', dispose: expect.any(Function) });
    expect(invoke).toHaveBeenCalledWith('chat_resolve_resource', { leaseId: 'lease-080', uri: 's3://bucket/image.png' });
    vi.clearAllMocks();
    expect(await port.resolve!({ resource: { uri: 'https://example.test/image.png' }, purpose: 'display', signal: signal() }))
      .toEqual({ url: 'https://example.test/image.png' });
    expect(invoke).not.toHaveBeenCalled();
    await expect(port.resolve!({ resource: { uri: 'file:///etc/passwd' }, purpose: 'display', signal: signal() })).rejects.toThrow();
  });

  it('rejects unsafe resolver responses and maps new public labels', async () => {
    vi.mocked(invoke).mockImplementation(async (name) => name === 'chat_resolve_resource'
      ? response({ url: 'javascript:alert(1)' }) : 'transfer-1');
    await expect(createChatResourcePort('lease-080').resolve!({
      resource: { uri: 's3://bucket/image.png' }, purpose: 'display', signal: signal(),
    })).rejects.toThrow();
    const labels = chatUiLabels((key) => key);
    expect(labels.attach).toBe('chat.workspace.attach');
    expect(labels.retryUpload).toBe('chat.workspace.retryUpload');
    expect(labels.pastedTextTitle).toBe('chat.workspace.pastedTextTitle');
    expect(labels.removeAttachment).toBe('chat.workspace.removeAttachment');
    expect(labels.removeLongText).toBe('chat.workspace.removeLongText');
  });

  it('renders the real composer and sends a selected image plus unabridged long paste', async () => {
    const uploader = factory().createAttachmentUploader!();
    const onSend = vi.fn(() => true);
    const view = render(createElement(ComposerHarness, { uploader, onSend }));
    try {
      const fileInput = view.container.querySelector('input[type="file"]')!;
      fireEvent.change(fileInput, { target: { files: [new File(['image bytes'], 'photo.png', { type: 'image/png' })] } });
      await screen.findByRole('button', { name: 'Remove attachment photo.png' });
      const longText = '长文本😀\n'.repeat(1000);
      fireEvent.paste(screen.getByRole('textbox'), { clipboardData: { files: [], getData: () => longText } });
      fireEvent.click(screen.getByRole('button', { name: 'Send' }));
      await waitFor(() => expect(onSend).toHaveBeenCalledWith(longText, [expect.objectContaining({
        uri: 's3://bucket/image.png', name: 'photo.png', mimeType: 'image/png',
      })]));
    } finally {
      view.unmount();
      await uploader.dispose?.();
    }
  });

  it('preserves real Markdown mail and relative-link opening without relaxing display or signed URLs', async () => {
    const port = createChatResourcePort('lease-080');
    const opened: string[] = [];
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
      opened.push(this.href);
    });
    const view = render(createElement(ChatResourceProvider, { port },
      createElement(ChatMarkdownContent, { children: '[Email](mailto:user@example.com) [Help](/help)' })));
    try {
      fireEvent.click(screen.getByRole('link', { name: 'Email' }));
      await waitFor(() => expect(opened).toContain('mailto:user@example.com'));
      fireEvent.click(screen.getByRole('link', { name: 'Help' }));
      await waitFor(() => expect(opened).toContain(new URL('/help', document.baseURI).href));
      expect(invoke).not.toHaveBeenCalled();
      for (const purpose of ['display', 'download'] as const) {
        await expect(port.resolve!({ resource: { uri: 'mailto:user@example.com' }, purpose, signal: signal() })).rejects.toThrow();
      }
      vi.mocked(invoke).mockImplementation(async (name) => name === 'chat_resolve_resource'
        ? response({ url: 'mailto:user@example.com' }) : 'transfer-1');
      await expect(port.resolve!({ resource: { uri: 's3://bucket/image.png' }, purpose: 'open', signal: signal() })).rejects.toThrow();
    } finally {
      view.unmount();
      click.mockRestore();
    }
  });
});
