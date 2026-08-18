export type MissingRuntimeInputError = {
  code: 'missing_input' | 'missing_secret' | 'invalid_selection';
  input_id: string;
  env_hint?: string;
  value?: string;
  message: string;
  requesting_mcp?: {
    bundle_id: string;
    name: string;
  };
};

export type MissingInputDefinitionError = {
  code: 'missing_input_definition';
  input_id: string;
  message: string;
  requesting_mcp?: {
    bundle_id: string;
    name: string;
  };
};

export type RuntimeActionError = MissingRuntimeInputError | MissingInputDefinitionError | {
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

export function isMissingInputDefinitionError(error: unknown): error is MissingInputDefinitionError {
  if (!error || typeof error !== 'object') return false;
  const candidate = error as Partial<MissingInputDefinitionError>;
  return candidate.code === 'missing_input_definition'
    && typeof candidate.input_id === 'string'
    && typeof candidate.message === 'string';
}

export function formatRuntimeActionError(error: unknown): string {
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as Partial<RuntimeActionError>).message;
    if (typeof message === 'string') return message;
  }
  return String(error);
}
