import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  PaymentRequiredError,
  RateLimitedError,
  ServerError,
  TokenExchangeError,
  TokenProfile,
} from '@turingfocus/tfrs-auth';
import {
  initializeManagerTokenBridge,
  managerTokenBridgeTestApi,
} from '@/services/managerTokenBridge';

const mockedInvoke = vi.mocked(invoke);
const mockedListen = vi.mocked(listen);

describe('managerTokenBridge', () => {
  const listeners = new Map<string, (event: { payload: unknown }) => void>();

  beforeEach(() => {
    vi.clearAllMocks();
    managerTokenBridgeTestApi.clearTokenSources();
    listeners.clear();
    mockedListen.mockImplementation(async (event, handler) => {
      listeners.set(event, handler as (event: { payload: unknown }) => void);
      return () => undefined;
    });
  });

  it('uses @turingfocus/tfrs-auth wire construction and reuses its generation-bound cache', async () => {
    const completions: Array<Record<string, unknown>> = [];
    const transportBodies: string[] = [];
    mockedInvoke.mockImplementation(async (command, args) => {
      if (command === 'manager_token_bridge_http_request') {
        const request = args as { body: string };
        transportBodies.push(request.body);
        return {
          status: 200,
          body: JSON.stringify({
            access_token: 'short-token',
            token_type: 'Bearer',
            expires_in: 300,
            scope: 'smcp:connect',
          }),
          contentType: 'application/json',
        };
      }
      if (command === 'manager_token_bridge_complete') {
        completions.push(args as Record<string, unknown>);
      }
      return undefined;
    });

    const dispose = await initializeManagerTokenBridge();
    const tokenListener = listeners.get('manager:token-request');
    expect(tokenListener).toBeDefined();
    const request = {
      generation: 7,
      tokenUrl: 'https://manager.example/api/v1/oauth/token',
      userJwt: 'user-jwt',
      audience: 'robot:r1',
      scope: 'smcp:connect',
      tokenProfile: TokenProfile.Session,
    };

    tokenListener?.({ payload: { ...request, requestId: 'request-1' } });
    await vi.waitFor(() => expect(completions).toHaveLength(1));
    const form = new URLSearchParams(transportBodies[0]);
    expect(Object.fromEntries(form.entries())).toEqual({
      grant_type: 'urn:ietf:params:oauth:grant-type:token-exchange',
      subject_token: 'user-jwt',
      subject_token_type: 'urn:ietf:params:oauth:token-type:jwt',
      audience: 'robot:r1',
      scope: 'smcp:connect',
      token_profile: 'session',
    });
    expect(completions[0]).toMatchObject({
      requestId: 'request-1',
      generation: 7,
      completion: {
        status: 'success',
        access_token: 'short-token',
        token_type: 'Bearer',
        scope: 'smcp:connect',
      },
    });

    tokenListener?.({ payload: { ...request, requestId: 'request-2' } });
    await vi.waitFor(() => expect(completions).toHaveLength(2));
    expect(transportBodies).toHaveLength(1);

    listeners.get('manager:context-changed')?.({ payload: {} });
    tokenListener?.({ payload: { ...request, requestId: 'request-3' } });
    await vi.waitFor(() => expect(completions).toHaveLength(3));
    expect(transportBodies).toHaveLength(2);

    await dispose();
  });

  it('shares one in-flight foundation exchange across concurrent native requests', async () => {
    let releaseTransport!: (response: ManagerTokenHttpResponseFixture) => void;
    const transport = new Promise<ManagerTokenHttpResponseFixture>((resolve) => {
      releaseTransport = resolve;
    });
    let transportCalls = 0;
    const completedRequestIds: string[] = [];
    mockedInvoke.mockImplementation(async (command, args) => {
      if (command === 'manager_token_bridge_http_request') {
        transportCalls += 1;
        return transport;
      }
      if (command === 'manager_token_bridge_complete') {
        completedRequestIds.push((args as { requestId: string }).requestId);
      }
      return undefined;
    });
    const dispose = await initializeManagerTokenBridge();
    const tokenListener = listeners.get('manager:token-request');
    const request = {
      generation: 11,
      tokenUrl: 'https://manager.example/api/v1/oauth/token',
      userJwt: 'user-jwt',
      audience: 'robot:r1',
      scope: 'smcp:connect',
      tokenProfile: TokenProfile.Session,
    };

    tokenListener?.({ payload: { ...request, requestId: 'concurrent-1' } });
    tokenListener?.({ payload: { ...request, requestId: 'concurrent-2' } });
    await vi.waitFor(() => expect(transportCalls).toBe(1));
    releaseTransport({
      status: 200,
      body: JSON.stringify({ access_token: 'shared', expires_in: 300 }),
      contentType: 'application/json',
    });
    await vi.waitFor(() => expect(completedRequestIds).toHaveLength(2));
    expect(completedRequestIds.sort()).toEqual(['concurrent-1', 'concurrent-2']);

    await dispose();
  });

  it('rejects a native request that does not select the session token profile', async () => {
    const completions: Array<Record<string, unknown>> = [];
    mockedInvoke.mockImplementation(async (command, args) => {
      if (command === 'manager_token_bridge_complete') {
        completions.push(args as Record<string, unknown>);
      }
      return undefined;
    });
    const dispose = await initializeManagerTokenBridge();

    listeners.get('manager:token-request')?.({
      payload: {
        requestId: 'invalid-profile',
        generation: 12,
        tokenUrl: 'https://manager.example/api/v1/oauth/token',
        userJwt: 'user-jwt',
        audience: 'robot:r1',
        tokenProfile: 'connection',
      },
    });

    await vi.waitFor(() => expect(completions).toHaveLength(1));
    expect(completions[0]).toMatchObject({
      completion: {
        status: 'error',
        error: { kind: 'invalid_response', description: 'Invalid Manager token profile' },
      },
    });
    expect(mockedInvoke).not.toHaveBeenCalledWith(
      'manager_token_bridge_http_request',
      expect.anything(),
    );
    await dispose();
  });

  it('keeps the shared bridge ready while an overlapping StrictMode owner remains', async () => {
    const readyCalls: Array<{ leaseId: string; ready: boolean }> = [];
    mockedInvoke.mockImplementation(async (command, args) => {
      if (command === 'manager_token_bridge_ready') {
        readyCalls.push(args as { leaseId: string; ready: boolean });
      }
      return undefined;
    });

    const [disposeFirst, disposeSecond] = await Promise.all([
      initializeManagerTokenBridge(),
      initializeManagerTokenBridge(),
    ]);
    expect(readyCalls.filter((call) => call.ready)).toHaveLength(1);

    await disposeFirst();
    expect(readyCalls.filter((call) => !call.ready)).toHaveLength(0);

    await disposeSecond();
    expect(readyCalls.filter((call) => !call.ready)).toHaveLength(1);
    expect(readyCalls[1].leaseId).toBe(readyCalls[0].leaseId);
  });

  it('redacts a reflected User JWT before sending an error completion', async () => {
    const completions: Array<Record<string, unknown>> = [];
    mockedInvoke.mockImplementation(async (command, args) => {
      if (command === 'manager_token_bridge_http_request') {
        return {
          status: 400,
          body: JSON.stringify({
            error: 'secret-user-jwt',
            error_description: 'rejected subject_token=secret-user-jwt',
          }),
          contentType: 'application/json',
        };
      }
      if (command === 'manager_token_bridge_complete') {
        completions.push(args as Record<string, unknown>);
      }
      return undefined;
    });
    const dispose = await initializeManagerTokenBridge();

    listeners.get('manager:token-request')?.({
      payload: {
        requestId: 'redaction-request',
        generation: 12,
        tokenUrl: 'https://manager.example/api/v1/oauth/token',
        userJwt: 'secret-user-jwt',
        audience: 'robot:r1',
        tokenProfile: TokenProfile.Session,
      },
    });

    await vi.waitFor(() => expect(completions).toHaveLength(1));
    const serialized = JSON.stringify(completions[0]);
    expect(serialized).not.toContain('secret-user-jwt');
    expect(serialized).toContain('[REDACTED]');
    await dispose();
  });

  it.each([
    [
      new PaymentRequiredError('renew', { renewUrl: 'https://pay' }),
      { kind: 'payment_required', description: 'renew', redirectUrl: 'https://pay', httpStatus: 402 },
    ],
    [
      new TokenExchangeError('invalid_scope', undefined, {
        httpStatus: 400,
        description: 'scope denied',
      }),
      { kind: 'token_exchange', code: 'invalid_scope', description: 'scope denied', httpStatus: 400 },
    ],
    [
      new RateLimitedError(undefined, { httpStatus: 429 }),
      { kind: 'rate_limited', httpStatus: 429 },
    ],
    [
      new ServerError(undefined, { httpStatus: 503, description: 'signer unavailable' }),
      {
        kind: 'signing_unavailable',
        code: 'server_error',
        description: 'signer unavailable',
        httpStatus: 503,
      },
    ],
  ])('maps typed foundation error %# into the stable native bridge contract', (error, expected) => {
    expect(managerTokenBridgeTestApi.toBridgeFailure(error)).toEqual(expected);
  });
});

interface ManagerTokenHttpResponseFixture {
  status: number;
  body: string;
  contentType?: string;
}
