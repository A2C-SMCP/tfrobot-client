import { useEffect, useRef, useState } from 'react';
import { Alert, Modal, Spin, Switch, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import { useInputStore, type InputDefinition, type InputEntry } from '@/stores/inputStore';
import type { MissingRuntimeInputError } from '@/utils/runtimeActionError';
import { InputValueEditor } from './InputValueEditor';

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
  const [storeAsSecret, setStoreAsSecret] = useState(error.code === 'missing_secret');
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
        setStoreAsSecret(entry?.secret ?? error.code === 'missing_secret');
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

  const handleSubmit = async (value: string) => {
    if (submittingRef.current) return;
    submittingRef.current = true;
    setSubmitError(false);
    try {
      await upsertEntry(instanceId, error.input_id, value, storeAsSecret);
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
      <Typography.Paragraph>{error.message}</Typography.Paragraph>
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
      {error.env_hint && (
        <Typography.Paragraph type="secondary">
          {t('inputs.runtime.envHint', { env: error.env_hint })}
        </Typography.Paragraph>
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
      {loaded?.definition && !loaded.entry && (
        <Alert
          type="info"
          showIcon
          message={t('inputs.runtime.entryMissing', { id: error.input_id })}
          style={{ marginBottom: 16 }}
        />
      )}
      {loaded && (
        <Typography.Paragraph>
          <Switch checked={storeAsSecret} onChange={setStoreAsSecret} style={{ marginRight: 8 }} />
          {t('inputs.runtime.storeAsSecret')}
        </Typography.Paragraph>
      )}
      {definition && (
        <InputValueEditor
          key={promptKey}
          inputId={error.input_id}
          inputs={[definition]}
          currentValue={error.code === 'invalid_selection' ? undefined : loaded?.entry?.value}
          onSubmit={handleSubmit}
          onCancel={onCancel}
        />
      )}
    </Modal>
  );
}
