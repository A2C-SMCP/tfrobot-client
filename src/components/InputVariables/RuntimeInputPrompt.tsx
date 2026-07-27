import { useEffect, useRef, useState } from 'react';
import { Alert, Modal, Spin, Switch, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import { useInputStore, type InputDefinition } from '@/stores/inputStore';
import type { MissingRuntimeInputError } from '@/utils/runtimeActionError';
import { InputValueEditor } from './InputValueEditor';

interface RuntimeInputPromptProps {
  instanceId: string;
  error: MissingRuntimeInputError;
  onCancel: () => void;
  onSubmitted: () => Promise<void>;
}

export function RuntimeInputPrompt({
  instanceId,
  error,
  onCancel,
  onSubmitted,
}: RuntimeInputPromptProps) {
  const { t } = useTranslation();
  const promptKey = `${instanceId}:${error.input_id}`;
  const getInput = useInputStore((state) => state.getInput);
  const addOrUpdateInput = useInputStore((state) => state.addOrUpdateInput);
  const setValue = useInputStore((state) => state.setValue);
  const [loadedDefinition, setLoadedDefinition] = useState<{
    key: string;
    value: InputDefinition | null;
  }>();
  const [loadError, setLoadError] = useState<string | null>(null);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [createAsSecret, setCreateAsSecret] = useState(error.code === 'missing_secret');
  const submittingRef = useRef(false);

  useEffect(() => {
    let active = true;
    setLoadedDefinition(undefined);
    setLoadError(null);
    setSubmitError(null);
    setCreateAsSecret(error.code === 'missing_secret');
    getInput(instanceId, error.input_id)
      .then((input) => {
        if (active) setLoadedDefinition({ key: promptKey, value: input });
      })
      .catch((cause) => {
        if (active) setLoadError(String(cause));
      });
    return () => {
      active = false;
    };
  }, [error.code, error.input_id, getInput, instanceId, promptKey]);

  const definition = loadedDefinition?.key === promptKey
    ? loadedDefinition.value
    : undefined;

  const handleSubmit = async (value: string) => {
    if (submittingRef.current) return;
    submittingRef.current = true;
    setSubmitError(null);
    try {
      if (definition === null) {
        await addOrUpdateInput(instanceId, {
          type: 'PromptString',
          id: error.input_id,
          label: error.input_id,
          description: error.message,
          password: createAsSecret,
        });
      }
      await setValue(instanceId, error.input_id, value);
      await onSubmitted();
    } catch (cause) {
      setSubmitError(String(cause));
    } finally {
      submittingRef.current = false;
    }
  };

  return (
    <Modal
      title={t(error.code === 'missing_secret' ? 'inputs.runtime.missingSecret' : 'inputs.runtime.missingInput')}
      open
      onCancel={onCancel}
      footer={null}
      destroyOnHidden
      width={440}
    >
      <Typography.Paragraph>{error.message}</Typography.Paragraph>
      <Typography.Paragraph type="secondary">
        {t('inputs.runtime.envHint', { env: error.env_hint })}
      </Typography.Paragraph>
      {loadError && <Alert type="error" showIcon message={loadError} />}
      {submitError && <Alert type="error" showIcon message={submitError} />}
      {definition === undefined && <Spin />}
      {definition === null && (
        <>
          <Alert
            type="info"
            showIcon
            message={t('inputs.runtime.definitionMissing', { id: error.input_id })}
            style={{ marginBottom: 16 }}
          />
          <Typography.Paragraph>
            <Switch
              checked={createAsSecret}
              onChange={setCreateAsSecret}
              style={{ marginRight: 8 }}
            />
            {t('inputs.runtime.createAsSecret')}
          </Typography.Paragraph>
        </>
      )}
      {definition !== undefined && (
        <InputValueEditor
          key={promptKey}
          inputId={error.input_id}
          inputs={[definition ?? {
            type: 'PromptString',
            id: error.input_id,
            label: error.input_id,
            description: error.message,
            password: createAsSecret,
          }]}
          currentValue={undefined}
          onSubmit={handleSubmit}
          onCancel={onCancel}
        />
      )}
    </Modal>
  );
}
