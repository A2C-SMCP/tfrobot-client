import { invoke } from '@tauri-apps/api/core';

export interface ChatHttpResponse {
  status: number;
  body: string;
  contentType?: string;
}

export const MAX_CHAT_FILE_BYTES = 10 * 1024 * 1024;

function abortError(): DOMException {
  return new DOMException('The chat request was aborted', 'AbortError');
}

async function invokeWithAbort<T>(promise: Promise<T>, signal: AbortSignal): Promise<T> {
  if (signal.aborted) {
    void promise.catch(() => undefined);
    throw abortError();
  }
  return new Promise<T>((resolve, reject) => {
    const abort = () => reject(abortError());
    signal.addEventListener('abort', abort, { once: true });
    void promise.then(resolve, reject).finally(() => signal.removeEventListener('abort', abort));
  });
}

/** Server-generated registration makes cancel-before-execute safe across unordered IPC calls. */
export async function invokeChatTransfer(
  leaseId: string,
  command: 'chat_upload_request',
  args: Record<string, unknown>,
  signal: AbortSignal,
): Promise<ChatHttpResponse> {
  if (signal.aborted) throw abortError();
  const transferId = await invoke<string>('chat_prepare_transfer', { leaseId });
  const cancel = () => {
    void invoke('chat_cancel_transfer', { leaseId, transferId }).catch(() => undefined);
  };
  if (signal.aborted) {
    cancel();
    throw abortError();
  }
  signal.addEventListener('abort', cancel, { once: true });
  try {
    return await invokeWithAbort(invoke<ChatHttpResponse>(command, { ...args, leaseId, transferId }), signal);
  } finally {
    signal.removeEventListener('abort', cancel);
    // Also release reservations when an IPC call fails before Rust enters the handler.
    cancel();
  }
}

async function encodeFile(file: File) {
  if (file.size > MAX_CHAT_FILE_BYTES) {
    throw new Error('Attachment exceeds 10 MiB');
  }
  const bytes = new Uint8Array(await file.arrayBuffer());
  const chunks: string[] = [];
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    chunks.push(String.fromCharCode(...bytes.subarray(offset, offset + 0x8000)));
  }
  return {
    name: file.name,
    mimeType: file.type || 'application/octet-stream',
    dataBase64: btoa(chunks.join('')),
  };
}

type UploadWaiter = { start: () => void; cancel: () => void };
const uploadQueues = new Map<string, { active: number; waiting: UploadWaiter[] }>();

/** Limit file reads before arrayBuffer/base64 allocation, including concurrent uploader instances. */
function acquireUploadSlot(leaseId: string, signal: AbortSignal): Promise<() => void> {
  if (signal.aborted) return Promise.reject(abortError());
  const queue = uploadQueues.get(leaseId) ?? { active: 0, waiting: [] };
  uploadQueues.set(leaseId, queue);
  return new Promise((resolve, reject) => {
    const release = () => {
      queue.active -= 1;
      queue.waiting.shift()?.start();
      if (queue.active === 0 && queue.waiting.length === 0) uploadQueues.delete(leaseId);
    };
    const waiter: UploadWaiter = {
      start: () => {
        signal.removeEventListener('abort', waiter.cancel);
        queue.active += 1;
        resolve(release);
      },
      cancel: () => {
        const index = queue.waiting.indexOf(waiter);
        if (index >= 0) queue.waiting.splice(index, 1);
        reject(abortError());
      },
    };
    if (queue.active < 2) waiter.start();
    else if (queue.waiting.length >= 64) reject(new Error('Too many queued attachment uploads'));
    else {
      queue.waiting.push(waiter);
      signal.addEventListener('abort', waiter.cancel, { once: true });
    }
  });
}

/** Rust owns credentials/route headers; only typed file metadata crosses the upload boundary. */
export function createTauriChatFetch(leaseId: string): typeof globalThis.fetch {
  return async (input, init) => {
    const form = init?.body instanceof FormData ? init.body : undefined;
    const request = new Request(input, form === undefined ? init : { ...init, body: undefined });
    if (request.signal.aborted) throw abortError();
    const isMultipart = form !== undefined
      || request.headers.get('content-type')?.toLowerCase().startsWith('multipart/form-data');
    let response: ChatHttpResponse;
    if (isMultipart) {
      if (request.method !== 'POST') throw new Error('Chat file upload requires POST');
      const data = form ?? await request.formData();
      const entries = [...data.entries()];
      const file = data.get('file');
      if (entries.length !== 1 || file === null || typeof file === 'string') {
        throw new Error('Chat upload requires exactly one file');
      }
      const release = await acquireUploadSlot(leaseId, request.signal);
      try {
        if (request.signal.aborted) throw abortError();
        const encoded = await encodeFile(file);
        response = await invokeChatTransfer(leaseId, 'chat_upload_request', {
          url: request.url,
          file: encoded,
        }, request.signal);
      } finally { release(); }
    } else {
      const body = request.method === 'GET' || request.method === 'HEAD' ? undefined : await request.text();
      if (request.signal.aborted) throw abortError();
      response = await invokeWithAbort(invoke<ChatHttpResponse>('chat_http_request', {
        leaseId, url: request.url, method: request.method, body,
      }), request.signal);
    }
    const headers = new Headers();
    if (response.contentType) headers.set('Content-Type', response.contentType);
    return new Response(response.body, { status: response.status, headers });
  };
}
