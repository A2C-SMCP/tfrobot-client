import type { HttpServerConfig } from '@/stores/mcpStore';

type HttpAuthenticationOptions = Pick<HttpServerConfig, 'oauth' | 'authPolicy'>;

/**
 * Keep explicit authentication intent from imported or legacy configurations while leaving new
 * HTTP servers unconfigured so the SDK can negotiate OAuth from the runtime challenge.
 */
export function preserveHttpAuthenticationOptions(
  initialConfig?: HttpServerConfig,
): HttpAuthenticationOptions {
  if (!initialConfig) return {};
  const preserved: HttpAuthenticationOptions = {};
  if (initialConfig.oauth !== undefined) preserved.oauth = initialConfig.oauth;
  if (initialConfig.authPolicy !== undefined) preserved.authPolicy = initialConfig.authPolicy;
  return preserved;
}

/** Match the SDK's case-insensitive detection of a static Authorization header. */
export function hasStaticAuthorizationHeader(headers: Record<string, string>): boolean {
  return Object.keys(headers).some((key) => key.toLowerCase() === 'authorization');
}

/**
 * The SDK treats a legacy OAuth block without a policy, and an explicit `oauth` policy, as
 * proactive OAuth. Automatic discovery and disabled OAuth may still use static credentials.
 */
export function usesProactiveOAuth(options: HttpAuthenticationOptions): boolean {
  return options.oauth != null
    && (options.authPolicy === undefined || options.authPolicy === 'oauth');
}

export function hasConflictingHttpAuthorization(
  options: HttpAuthenticationOptions,
  headers: Record<string, string>,
): boolean {
  return usesProactiveOAuth(options) && hasStaticAuthorizationHeader(headers);
}
