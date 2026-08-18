import { useEffect, useRef, useState } from 'react';
import { Alert, Modal, Spin, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import { useInputStore, type InputDefinition, type InputEntry } from '@/stores/inputStore';
import type { MissingRuntimeInputError } from '@/utils/runtimeActionError';
import { InputEntryEditor } from './InputEntryEditor';

interface RuntimeInputPromptProps {
  instanceId: string;
  error: MissingRuntimeInputError;
  onCancel: () => void;
  onSubmitted: () => Promise<void>;
}

type LoadedContext = {
  key: string;
  definition: InputDefinition | null;
  entry: InputEntry | null;
};

export function RuntimeInputPrompt({ instanceId, error, onCancel, onSubmitted }: RuntimeInputPromptProps) {
  const { t } = useTranslation();
  const promptKey = `${instanceId}:${error.input_id}`;
  const getRuntimeInput = useInputStore((state) => state.getRuntimeInput);
  const getEntry = useInputStore((state) => state.getEntry);
  const upsertEntry = useInputStore((state) => state.upsertEntry);
  const [context, setContext] = useState<LoadedContext>();
  const [loadError, setLoadError] = useState(false);
  const [submitError, setSubmitError] = useState(false);
  const submittingRef = useRef(false);

  useEffect(() => {
    let active = true;
    setContext(undefined);
    setLoadError(false);
    setSubmitError(false);
    Promise.all([
      getRuntimeInput(instanceId, error.input_id),
      getEntry(instanceId, error.input_id),
    ])
      .then(([definition, entry]) => {
        if (!active) return;
        setContext({ key: promptKey, definition, entry });
      })
      .catch(() => {
        if (active) setLoadError(true);
      });
    return () => {
      active = false;
    };
  }, [error.code, error.input_id, getEntry, getRuntimeInput, instanceId, promptKey]);

  const loaded = context?.key === promptKey ? context : undefined;
  const definition = loaded?.definition;
  const editorEntry = loaded?.entry
    ? {
      ...loaded.entry,
      value: error.code === 'invalid_selection' ? undefined : loaded.entry.value,
    }
    : undefined;

  const handleSubmit = async (key: string, value: string | undefined, secret: boolean) => {
    if (submittingRef.current) return;
    submittingRef.current = true;
    setSubmitError(false);
    try {
      await upsertEntry(instanceId, key, value, secret);
      await onSubmitted();
    } catch {
      setSubmitError(true);
    } finally {
      submittingRef.current = false;
    }
  };

  return (
    <Modal
      title={error.code === 'invalid_selection'
        ? t(
          error.value !== undefined ? 'inputs.status.invalidSelection' : 'inputs.status.invalidSecretSelection',
          { value: error.value },
        )
        : t(error.code === 'missing_secret' ? 'inputs.runtime.missingSecret' : 'inputs.runtime.missingInput')}
      open
      onCancel={onCancel}
      footer={null}
      destroyOnHidden
      width={440}
    >
      <Typography.Paragraph type="secondary">
        {t('inputs.runtime.description')}
      </Typography.Paragraph>
      {error.requesting_mcp && (
        <Alert
          type="info"
          showIcon
          message={t('inputs.runtime.requestedBy', {
            name: error.requesting_mcp.name,
            id: error.requesting_mcp.bundle_id,
          })}
          style={{ marginBottom: 16 }}
        />
      )}
      {loadError && <Alert type="error" showIcon message={t('inputs.runtime.loadFailed')} />}
      {submitError && <Alert type="error" showIcon message={t('inputs.runtime.submitFailed')} />}
      {!loaded && !loadError && <Spin />}
      {loaded && !loaded.definition && (
        <Alert
          type="error"
          showIcon
          message={t('inputs.runtime.definitionUnavailable', { id: error.input_id })}
          style={{ marginBottom: 16 }}
        />
      )}
      {definition && (
        <InputEntryEditor
          key={promptKey}
          entry={editorEntry}
          fixedKey={error.input_id}
          definition={definition}
          initialSecret={error.code === 'missing_secret'}
          requireValue={Boolean(editorEntry && editorEntry.value === undefined)}
          onSubmit={handleSubmit}
          onCancel={onCancel}
        />
      )}
    </Modal>
  );
}
