export type MissingRuntimeInputError = {
  code: 'missing_input' | 'missing_secret' | 'invalid_selection';
  input_id: string;
  env_hint?: string;
  value?: string;
  message: string;
};

export type RuntimeActionError = MissingRuntimeInputError | {
  code: 'resolver_failed' | 'runtime_error';
  input_id?: string;
  message: string;
} | {
  code: 'action_unavailable';
  action: string;
  lifecycle: string;
  disabled_reason: string;
  message: string;
};

export function isMissingRuntimeInputError(error: unknown): error is MissingRuntimeInputError {
  if (!error || typeof error !== 'object') return false;
  const candidate = error as Partial<MissingRuntimeInputError>;
  return (candidate.code === 'missing_input'
    || candidate.code === 'missing_secret'
    || candidate.code === 'invalid_selection')
    && typeof candidate.input_id === 'string'
    && (candidate.code === 'invalid_selection' || typeof candidate.env_hint === 'string')
    && typeof candidate.message === 'string';
}

export function formatRuntimeActionError(error: unknown): string {
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as Partial<RuntimeActionError>).message;
    if (typeof message === 'string') return message;
  }
  return String(error);
}
