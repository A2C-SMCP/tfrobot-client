import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  CachingTokenSource,
  PaymentRequiredError,
  TokenExchangeError,
  TransportError,
  UserJwtCredential,
  type TokenSource,
} from '@turingfocus/tfrs-auth';

const TOKEN_REQUEST_EVENT = 'manager:token-request';
const CONTEXT_CHANGED_EVENT = 'manager:context-changed';
const AUTH_EXPIRED_EVENT = 'manager:auth-expired';
// Rust schedules renewal from a floored `expiresIn` value at roughly T-60s. Keep a small margin
// above that boundary so the scheduled call cannot receive the still-cached token due to rounding.
const TOKEN_EXPIRY_SKEW_SECONDS = 65;

interface ManagerTokenRequest {
  requestId: string;
  generation: number;
  tokenUrl: string;
  userJwt: string;
  audience: string;
  scope?: string | null;
}

interface ManagerTokenHttpResponse {
  status: number;
  body: string;
  contentType?: string | null;
}

interface BridgeFailure {
  kind: string;
  code?: string;
  description?: string;
  httpStatus?: number;
  redirectUrl?: string;
}

type BridgeCompletion = {
  status: 'success';
  access_token: string;
  token_type: string;
  expires_in: number;
  scope?: string;
} | {
  status: 'error';
  error: BridgeFailure;
};

interface NativeManagerError {
  kind?: unknown;
  detail?: unknown;
}

interface TokenSourceEntry {
  source: TokenSource;
  currentRequest: ManagerTokenRequest;
}

const sources = new Map<string, TokenSourceEntry>();
let latestGeneration: number | null = null;
let bridgeLeaseSequence = 0;

interface BridgeSession {
  leaseId: string;
  references: number;
  unlisteners: UnlistenFn[];
  start: Promise<void>;
  shutdown: Promise<void> | null;
}

let activeBridgeSession: BridgeSession | null = null;

function clearTokenSources(): void {
  for (const entry of sources.values()) entry.source.invalidate();
  sources.clear();
  latestGeneration = null;
}

function sourceKey(request: ManagerTokenRequest): string {
  return JSON.stringify([
    request.generation,
    request.tokenUrl,
    request.audience,
    request.scope ?? '',
  ]);
}

function redactSecret(value: string | undefined, secret: string): string | undefined {
  if (!value || !secret) return value;
  return value.split(secret).join('[REDACTED]');
}

function nativeFailure(value: unknown, secret: string): BridgeFailure | null {
  if (typeof value !== 'object' || value === null) return null;
  const candidate = value as NativeManagerError;
  if (typeof candidate.kind !== 'string') return null;
  const description = typeof candidate.detail === 'string'
    ? redactSecret(candidate.detail, secret)
    : undefined;
  switch (candidate.kind) {
    case 'context_changed':
    case 'no_session':
    case 'unauthorized':
    case 'network_error':
    case 'invalid_response':
      return { kind: candidate.kind, ...(description ? { description } : {}) };
    default:
      return { kind: 'invalid_response', description: 'Native token transport failed' };
  }
}

function toBridgeFailure(error: unknown, secret = ''): BridgeFailure {
  if (error instanceof PaymentRequiredError) {
    return {
      kind: 'payment_required',
      description: redactSecret(error.description ?? error.message, secret),
      ...(error.renewUrl ? { redirectUrl: redactSecret(error.renewUrl, secret) } : {}),
      httpStatus: 402,
    };
  }
  if (error instanceof TokenExchangeError) {
    const status = error.httpStatus;
    if (status === 401) return { kind: 'unauthorized', httpStatus: status };
    if (status === 403) return { kind: 'forbidden', httpStatus: status };
    if (status === 404) return { kind: 'not_found', httpStatus: status };
    if (status === 429) return { kind: 'rate_limited', httpStatus: status };
    if (status === 503) {
      return {
        kind: 'signing_unavailable',
        code: redactSecret(error.code, secret),
        description: redactSecret(error.description, secret),
        httpStatus: status,
      };
    }
    if (status !== undefined && status >= 500) {
      return {
        kind: 'other',
        code: redactSecret(error.code, secret),
        description: redactSecret(error.description, secret),
        httpStatus: status,
      };
    }
    return {
      kind: 'token_exchange',
      code: redactSecret(error.code, secret),
      description: redactSecret(error.description, secret),
      ...(status === undefined ? {} : { httpStatus: status }),
    };
  }
  if (error instanceof TransportError) {
    const cause = nativeFailure((error as Error & { cause?: unknown }).cause, secret);
    return cause ?? { kind: 'network_error', description: 'Token endpoint request failed' };
  }
  return nativeFailure(error, secret)
    ?? { kind: 'invalid_response', description: 'TypeScript token bridge failed' };
}

function bridgeFetch(currentRequest: () => ManagerTokenRequest): typeof globalThis.fetch {
  return async (input, init) => {
    const request = currentRequest();
    const outgoing = new Request(input, init);
    const body = await outgoing.text();
    const response = await invoke<ManagerTokenHttpResponse>('manager_token_bridge_http_request', {
      requestId: request.requestId,
      generation: request.generation,
      body,
    });
    const headers = new Headers();
    if (response.contentType) headers.set('content-type', response.contentType);
    return new Response(response.body, { status: response.status, headers });
  };
}

function tokenSource(request: ManagerTokenRequest): TokenSource {
  const key = sourceKey(request);
  const existing = sources.get(key);
  if (existing) {
    existing.currentRequest = request;
    return existing.source;
  }
  const entry = {} as TokenSourceEntry;
  const source = new CachingTokenSource(
    new UserJwtCredential({
      userJwt: request.userJwt,
      audience: request.audience,
      ...(request.scope ? { scope: request.scope } : {}),
    }),
    {
      tokenUrl: request.tokenUrl,
      fetch: bridgeFetch(() => entry.currentRequest),
      expirySkewSeconds: TOKEN_EXPIRY_SKEW_SECONDS,
    },
  );
  entry.source = source;
  entry.currentRequest = request;
  sources.set(key, entry);
  return source;
}

async function complete(request: ManagerTokenRequest, completion: BridgeCompletion): Promise<void> {
  await invoke('manager_token_bridge_complete', {
    requestId: request.requestId,
    generation: request.generation,
    completion,
  });
}

async function handleTokenRequest(request: ManagerTokenRequest): Promise<void> {
  if (!Number.isSafeInteger(request.generation) || request.generation < 0) {
    await complete(request, {
      status: 'error',
      error: { kind: 'invalid_response', description: 'Invalid Manager generation' },
    });
    return;
  }
  if (latestGeneration !== null && request.generation < latestGeneration) {
    await complete(request, { status: 'error', error: { kind: 'context_changed' } });
    return;
  }
  if (latestGeneration !== request.generation) {
    clearTokenSources();
    latestGeneration = request.generation;
  }
  try {
    const token = await tokenSource(request).token();
    await complete(request, {
      status: 'success',
      access_token: token.accessToken,
      token_type: token.tokenType,
      expires_in: Math.max(0, Math.floor(token.expiresIn())),
      ...(token.scope ? { scope: token.scope } : {}),
    });
  } catch (error) {
    await complete(request, {
      status: 'error',
      error: toBridgeFailure(error, request.userJwt),
    });
  }
}

async function startBridgeSession(session: BridgeSession): Promise<void> {
  try {
    session.unlisteners.push(await listen<ManagerTokenRequest>(TOKEN_REQUEST_EVENT, (event) => {
      void handleTokenRequest(event.payload).catch(() => {
        // Native timeout and generation guards settle requests if the completion command fails.
      });
    }));
    session.unlisteners.push(await listen(CONTEXT_CHANGED_EVENT, clearTokenSources));
    session.unlisteners.push(await listen(AUTH_EXPIRED_EVENT, clearTokenSources));
    await invoke('manager_token_bridge_ready', { leaseId: session.leaseId, ready: true });
  } catch (error) {
    for (const unlisten of session.unlisteners) unlisten();
    session.unlisteners.length = 0;
    clearTokenSources();
    throw error;
  }
}

function createBridgeSession(): BridgeSession {
  const session = {
    leaseId: `manager-token-bridge-${++bridgeLeaseSequence}`,
    references: 0,
    unlisteners: [],
    start: Promise.resolve(),
    shutdown: null,
  } satisfies BridgeSession;
  session.start = startBridgeSession(session);
  return session;
}

export async function initializeManagerTokenBridge(): Promise<() => Promise<void>> {
  if (activeBridgeSession?.shutdown) {
    await activeBridgeSession.shutdown;
    return initializeManagerTokenBridge();
  }
  const session = activeBridgeSession ?? createBridgeSession();
  activeBridgeSession = session;
  session.references += 1;
  try {
    await session.start;
  } catch (error) {
    session.references -= 1;
    if (session.references === 0 && activeBridgeSession === session) {
      activeBridgeSession = null;
    }
    throw error;
  }

  let disposed = false;
  return async () => {
    if (disposed) return;
    disposed = true;
    session.references -= 1;
    if (session.references > 0) return;
    if (!session.shutdown) {
      session.shutdown = (async () => {
        clearTokenSources();
        try {
          await invoke('manager_token_bridge_ready', {
            leaseId: session.leaseId,
            ready: false,
          });
        } catch {
          // Native shutdown may already have removed the command channel.
        }
        for (const unlisten of session.unlisteners) unlisten();
        session.unlisteners.length = 0;
        if (activeBridgeSession === session) activeBridgeSession = null;
      })();
    }
    await session.shutdown;
  };
}

export const managerTokenBridgeTestApi = {
  clearTokenSources,
  toBridgeFailure,
};
