import { useEffect, useRef, useState } from 'react';
import { Alert, Modal, Typography } from 'antd';
import { useTranslation } from 'react-i18next';
import { useInputStore } from '@/stores/inputStore';
import { useRuntimeInputStore } from '@/stores/runtimeInputStore';
import {
  completeRuntimeInputRequest,
  RuntimeInputCompletionError,
} from '@/services/runtimeInputBridge';
import { InputEntryEditor } from './InputEntryEditor';

export function RuntimeInputPrompt() {
  const { t } = useTranslation();
  const request = useRuntimeInputStore((state) => state.requests[0]);
  const remove = useRuntimeInputStore((state) => state.remove);
  const reportCompletionFailure = useRuntimeInputStore((state) => state.reportCompletionFailure);
  const [cancelError, setCancelError] = useState(false);
  const submittingRef = useRef(false);

  useEffect(() => {
    setCancelError(false);
    submittingRef.current = false;
  }, [request?.requestId]);

  if (!request) return null;
  const definition = request.definition;
  const initialValue = definition.type === 'Command'
    || (definition.type === 'PromptString' && definition.password === true)
    ? undefined
    : definition.default;
  const requireValue = definition.type === 'PickString' && initialValue === undefined;

  const handleSubmit = async (_key: string, value: string | undefined) => {
    if (submittingRef.current || value === undefined) return;
    submittingRef.current = true;
    try {
      await completeRuntimeInputRequest(request.requestId, { status: 'confirmed', value });
      remove(request.requestId);
      const inputState = useInputStore.getState();
      if (inputState.activeInstanceId === request.instanceId) {
        await Promise.all([
          inputState.fetchEntries(request.instanceId),
          inputState.refreshValues(request.instanceId),
        ]);
      }
    } catch (error) {
      if (error instanceof RuntimeInputCompletionError && error.terminal) {
        remove(request.requestId);
        reportCompletionFailure();
      }
      throw error;
    } finally {
      submittingRef.current = false;
    }
  };

  const handleCancel = async () => {
    if (submittingRef.current) return;
    submittingRef.current = true;
    setCancelError(false);
    try {
      await completeRuntimeInputRequest(request.requestId, { status: 'cancelled' });
      remove(request.requestId);
    } catch (error) {
      if (error instanceof RuntimeInputCompletionError && error.terminal) {
        remove(request.requestId);
        reportCompletionFailure();
      } else {
        setCancelError(true);
      }
    } finally {
      submittingRef.current = false;
    }
  };

  return (
    <Modal
      title={request.reason === 'invalid_selection'
        ? t(request.secret ? 'inputs.status.invalidSecretSelection' : 'inputs.runtime.invalidSelection')
        : t(request.secret ? 'inputs.runtime.missingSecret' : 'inputs.runtime.missingInput')}
      open
      onCancel={() => { void handleCancel(); }}
      footer={null}
      destroyOnHidden
      width={440}
    >
      <Typography.Paragraph type="secondary">
        {t('inputs.runtime.description')}
      </Typography.Paragraph>
      {cancelError && <Alert type="error" showIcon message={t('inputs.runtime.submitFailed')} />}
      <InputEntryEditor
        key={request.requestId}
        fixedKey={definition.id}
        definition={definition}
        initialSecret={request.secret}
        initialValue={initialValue}
        lockSecret
        requireValue={requireValue}
        onSubmit={handleSubmit}
        onCancel={() => { void handleCancel(); }}
      />
    </Modal>
  );
}
