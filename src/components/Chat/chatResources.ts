import { invoke } from '@tauri-apps/api/core';
import { ChatResourceError, type ChatResourceErrorCode, type ChatResourcePort, type ChatResourceRequest, type ChatResolvedResource } from '@turingfocus/chat-kit/headless';

interface ResourceHandle { id: string; url: string }

function safeUrl(value: unknown, openingDirectLink = false): string {
  if (typeof value !== 'string') throw new Error('Chat resource is unavailable');
  const url = new URL(value, openingDirectLink ? document.baseURI : undefined);
  const allowed = ['https:', 'http:', 'blob:'].includes(url.protocol)
    || (openingDirectLink && url.protocol === 'mailto:');
  if (!allowed || url.username || url.password) throw new Error('Chat resource is unavailable');
  return url.href;
}

const cancelled = () => new DOMException('Resource request cancelled', 'AbortError');

// Keep native diagnostic codes at the host boundary; Kit only receives its public vocabulary.
const resourceErrorCodes = {
  permission: 'unauthorized', not_found: 'not-found', network: 'network',
  timeout: 'network', busy: 'network', unsupported: 'unsupported', cancelled: 'cancelled',
  invalid: 'unknown', too_large: 'unknown', save: 'unknown',
} as const satisfies Record<string, ChatResourceErrorCode>;

function nativeErrorCode(error: unknown): keyof typeof resourceErrorCodes | undefined {
  if (typeof error !== 'object' || error === null) return undefined;
  if ('name' in error && error.name === 'AbortError') return 'cancelled';
  if ('code' in error && typeof error.code === 'string'
    && Object.prototype.hasOwnProperty.call(resourceErrorCodes, error.code)) return error.code as keyof typeof resourceErrorCodes;
  return undefined;
}

/** The only URLs created for private resources are revocable Rust loopback capabilities. */
export function createChatResourcePort(leaseId: string, onError?: (code: string) => void): ChatResourcePort {
  async function register(request: ChatResourceRequest): Promise<ChatResolvedResource & { id: string }> {
    if (request.signal.aborted) throw cancelled();
    const handle = await invoke<ResourceHandle>('chat_resolve_resource', { leaseId, uri: request.resource.uri });
    let released = false;
    let unsubscribe = () => undefined as void;
    const dispose = () => {
      if (released) return;
      released = true;
      unsubscribe();
      void invoke('chat_release_resource', { leaseId, resourceId: handle.id }).catch(() => undefined);
    };
    try {
      const url = new URL(safeUrl(handle.url));
      if (url.protocol !== 'http:' || url.hostname !== '127.0.0.1' || !url.port
        || url.pathname !== `/resource/${handle.id}` || url.search || url.hash || !handle.id) {
        throw new Error('Invalid resource capability');
      }
      unsubscribe = request.signal.subscribe(dispose);
      if (request.signal.aborted) { dispose(); throw cancelled(); }
      return { id: handle.id, url: url.href, dispose };
    } catch (error) { dispose(); throw error; }
  }

  async function action(request: ChatResourceRequest): Promise<void> {
    if (request.signal.aborted) throw cancelled();
    if (!request.resource.uri.startsWith('s3://')) {
      const anchor = document.createElement('a');
      anchor.href = safeUrl(request.resource.uri, request.purpose === 'open');
      anchor.target = '_blank';
      anchor.rel = 'noopener noreferrer';
      if (request.purpose === 'download') anchor.download = request.resource.name ?? 'download';
      anchor.click();
      return;
    }
    let handle: Awaited<ReturnType<typeof register>> | undefined;
    try {
      handle = await register(request);
      if (request.signal.aborted) throw cancelled();
      await invoke('chat_save_resource', {
        leaseId, resourceId: handle.id, name: request.resource.name, open: request.purpose === 'open',
      });
    } finally { handle?.dispose?.(); }
  }

  async function withResourceError<T>(request: ChatResourceRequest, operation: () => Promise<T>): Promise<T> {
    try {
      return await operation();
    } catch (error) {
      const code = request.signal.aborted ? 'cancelled' : nativeErrorCode(error);
      // Display failures also arrive through the native media error event. Actions need
      // host details for errors Kit cannot describe (disk, size, queue limits).
      if (request.purpose !== 'display' && code !== 'cancelled') onError?.(code ?? 'network');
      throw new ChatResourceError(code === undefined ? 'unknown' : resourceErrorCodes[code]);
    }
  }

  return {
    resolve: (request) => withResourceError(request, async () => {
      if (request.signal.aborted) throw cancelled();
      if (!request.resource.uri.startsWith('s3://')) return { url: safeUrl(request.resource.uri, request.purpose === 'open') };
      const resolved = await register(request);
      return { url: resolved.url, dispose: resolved.dispose };
    }),
    open: (request) => withResourceError(request, () => action(request)),
    download: (request) => withResourceError(request, () => action(request)),
  };
}
