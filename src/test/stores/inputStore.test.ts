import { invoke } from '@tauri-apps/api/core';
import { useInputStore, type InputDefinition, type InputEntry, type InputValueView } from '@/stores/inputStore';

const mockedInvoke = vi.mocked(invoke);
const instanceId = 'computer-a';

function resetStore() {
  useInputStore.setState({
    inputs: [],
    entries: [],
    values: {},
    loading: false,
    error: null,
    valuesLoading: false,
    valuesLoadedInstanceId: null,
    valuesError: null,
    activeInstanceId: null,
    inputsRequestId: 0,
    valuesRequestId: 0,
    entriesLoading: false,
    entriesLoadedInstanceId: null,
    entriesError: null,
    entriesRequestId: 0,
  });
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe('inputStore', () => {
  beforeEach(() => {
    resetStore();
    mockedInvoke.mockReset();
  });

  it('keeps existing entries visible while refreshing the same Computer', async () => {
    const entries: InputEntry[] = [{ key: 'name', value: 'retained', secret: false }];
    useInputStore.setState({ entries, entriesLoadedInstanceId: instanceId });
    const pending = deferred<InputEntry[]>();
    mockedInvoke.mockReturnValueOnce(pending.promise);
    const refresh = useInputStore.getState().fetchEntries(instanceId);
    expect(useInputStore.getState().entries).toEqual(entries);
    pending.resolve(entries);
    await refresh;
  });

  describe('fetchInputs', () => {
    it('populates inputs list', async () => {
      const mockInputs: InputDefinition[] = [
        { type: 'PromptString', id: 'api_key', label: 'API Key' },
      ];
      mockedInvoke.mockResolvedValueOnce(mockInputs);

      await useInputStore.getState().fetchInputs(instanceId);

      expect(mockedInvoke).toHaveBeenCalledWith('list_inputs', { instanceId });
      expect(useInputStore.getState().inputs).toEqual(mockInputs);
      expect(useInputStore.getState().loading).toBe(false);
    });

    it('sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('load failed');

      await useInputStore.getState().fetchInputs(instanceId);

      expect(useInputStore.getState().error).toBe('load failed');
      expect(useInputStore.getState().loading).toBe(false);
    });

    it('ignores stale input definitions from a previous computer instance', async () => {
      const first = deferred<InputDefinition[]>();
      const second = deferred<InputDefinition[]>();
      const inputsA: InputDefinition[] = [
        { type: 'PromptString', id: 'api_key_a', label: 'API Key A' },
      ];
      const inputsB: InputDefinition[] = [
        { type: 'PromptString', id: 'api_key_b', label: 'API Key B' },
      ];
      mockedInvoke.mockReturnValueOnce(first.promise as any);
      mockedInvoke.mockReturnValueOnce(second.promise as any);

      const firstFetch = useInputStore.getState().fetchInputs('computer-a');
      const secondFetch = useInputStore.getState().fetchInputs('computer-b');

      second.resolve(inputsB);
      await secondFetch;
      first.resolve(inputsA);
      await firstFetch;

      expect(useInputStore.getState().inputs).toEqual(inputsB);
      expect(useInputStore.getState().activeInstanceId).toBe('computer-b');
      expect(useInputStore.getState().loading).toBe(false);
    });
  });

  describe('InputEntry management', () => {
    it('loads only actual saved entries', async () => {
      const entries: InputEntry[] = [
        { key: 'name', secret: false, value: 'zhangsan' },
        { key: 'api-key', secret: true },
      ];
      mockedInvoke.mockResolvedValueOnce(entries);

      await useInputStore.getState().fetchEntries(instanceId);

      expect(mockedInvoke).toHaveBeenCalledWith('list_input_entries', { instanceId });
      expect(useInputStore.getState().entries).toEqual(entries);
    });

    it('upserts an entry without an SDK definition and refreshes entries', async () => {
      useInputStore.setState({ activeInstanceId: instanceId });
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce([{ key: 'name', secret: false, value: 'zhangsan' }]);

      await useInputStore.getState().upsertEntry(instanceId, 'name', 'zhangsan', false);

      expect(mockedInvoke).toHaveBeenCalledWith('upsert_input_entry', {
        instanceId,
        key: 'name',
        value: 'zhangsan',
        secret: false,
      });
    });

    it('sends null when editing a secret without replacing its plaintext', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);
      await useInputStore.getState().upsertEntry(instanceId, 'api-key', undefined, true);
      expect(mockedInvoke).toHaveBeenCalledWith('upsert_input_entry', {
        instanceId,
        key: 'api-key',
        value: null,
        secret: true,
      });
    });

    it('deletes one entry and refreshes the actual-entry list', async () => {
      useInputStore.setState({ activeInstanceId: instanceId });
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce([]);
      await useInputStore.getState().deleteEntry(instanceId, 'name');
      expect(mockedInvoke).toHaveBeenCalledWith('delete_input_entry', {
        instanceId,
        key: 'name',
      });
    });
  });

  describe('fetchValues', () => {
    it('populates values map', async () => {
      const mockValues = { api_key: { configured: true, value: 'secret123' } };
      mockedInvoke.mockResolvedValueOnce(mockValues);

      await useInputStore.getState().fetchValues(instanceId);

      expect(mockedInvoke).toHaveBeenCalledWith('list_input_values', { instanceId });
      expect(useInputStore.getState().values).toEqual(mockValues);
      expect(useInputStore.getState().valuesLoadedInstanceId).toBe(instanceId);
      expect(useInputStore.getState().valuesLoading).toBe(false);
    });

    it('ignores stale input values from a previous computer instance', async () => {
      const first = deferred<Record<string, InputValueView>>();
      const second = deferred<Record<string, InputValueView>>();
      mockedInvoke.mockReturnValueOnce(first.promise as any);
      mockedInvoke.mockReturnValueOnce(second.promise as any);

      const firstFetch = useInputStore.getState().fetchValues('computer-a');
      const secondFetch = useInputStore.getState().fetchValues('computer-b');

      second.resolve({ token: { configured: true, status: 'configured' } });
      await secondFetch;
      first.resolve({ token: { configured: true, status: 'configured' } });
      await firstFetch;

      expect(useInputStore.getState().values).toEqual({ token: { configured: true, status: 'configured' } });
      expect(useInputStore.getState().activeInstanceId).toBe('computer-b');
    });

    it('keeps definitions and values scoped during a component-style instance switch', async () => {
      const inputsA = deferred<InputDefinition[]>();
      const inputsB = deferred<InputDefinition[]>();
      const valuesB = deferred<Record<string, InputValueView>>();
      const definitionsB: InputDefinition[] = [
        { type: 'PromptString', id: 'token_b', label: 'Token B' },
      ];
      mockedInvoke.mockReturnValueOnce(inputsA.promise as any);
      mockedInvoke.mockReturnValueOnce(inputsB.promise as any);
      mockedInvoke.mockReturnValueOnce(valuesB.promise as any);

      const firstInputsFetch = useInputStore.getState().fetchInputs('computer-a');
      const secondInputsFetch = useInputStore.getState().fetchInputs('computer-b');
      const secondValuesFetch = useInputStore.getState().fetchValues('computer-b');

      inputsB.resolve(definitionsB);
      valuesB.resolve({ token_b: { configured: true, status: 'configured' } });
      await secondInputsFetch;
      await secondValuesFetch;

      inputsA.resolve([{ type: 'PromptString', id: 'token_a', label: 'Token A' }]);
      await firstInputsFetch;

      expect(useInputStore.getState().inputs).toEqual(definitionsB);
      expect(useInputStore.getState().values).toEqual({ token_b: { configured: true, status: 'configured' } });
      expect(useInputStore.getState().activeInstanceId).toBe('computer-b');
      expect(useInputStore.getState().loading).toBe(false);
      expect(useInputStore.getState().error).toBeNull();
    });
  });

  describe('addOrUpdateInput', () => {
    it('invokes add_or_update_input and refreshes', async () => {
      const input: InputDefinition = { type: 'PromptString', id: 'token', label: 'Token' };
      useInputStore.setState({ activeInstanceId: instanceId });
      mockedInvoke.mockResolvedValueOnce(undefined); // add_or_update_input
      mockedInvoke.mockResolvedValueOnce([input]);   // fetchInputs

      await useInputStore.getState().addOrUpdateInput(instanceId, input);

      expect(mockedInvoke).toHaveBeenCalledWith('add_or_update_input', { instanceId, input });
      expect(useInputStore.getState().inputs).toEqual([input]);
    });

    it('sets error and re-throws on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('save failed');

      await expect(
        useInputStore.getState().addOrUpdateInput(instanceId, { type: 'PromptString', id: 'x', label: 'X' })
      ).rejects.toBe('save failed');

      expect(useInputStore.getState().error).toBe('save failed');
    });
  });

  describe('removeInput', () => {
    it('invokes remove_input and refreshes inputs + values', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined); // remove_input
      mockedInvoke.mockResolvedValueOnce([]);         // fetchInputs
      mockedInvoke.mockResolvedValueOnce({});          // fetchValues
      mockedInvoke.mockResolvedValueOnce([]);          // fetchReferenceIssues

      await useInputStore.getState().removeInput(instanceId, 'api_key');

      expect(mockedInvoke).toHaveBeenCalledWith('remove_input', { instanceId, id: 'api_key' });
    });
  });

  describe('setValue', () => {
    it('invokes set_input_value and refreshes values', async () => {
      useInputStore.setState({ activeInstanceId: instanceId });
      mockedInvoke.mockResolvedValueOnce(undefined);     // set_input_value
      mockedInvoke.mockResolvedValueOnce({ key: 'val' }); // fetchValues

      await useInputStore.getState().setValue(instanceId, 'key', 'val');

      expect(mockedInvoke).toHaveBeenCalledWith('set_input_value', { instanceId, id: 'key', value: 'val' });
    });

    it('does not let a completed mutation reclaim the active Computer', async () => {
      const mutationA = deferred<void>();
      const inputsB = deferred<InputDefinition[]>();
      const valuesB = deferred<Record<string, InputValueView>>();
      useInputStore.setState({
        activeInstanceId: 'computer-a',
        valuesLoadedInstanceId: 'computer-a',
        values: { token_a: { configured: true, status: 'configured', value: 'old-a' } },
      });
      mockedInvoke.mockReturnValueOnce(mutationA.promise as any);
      mockedInvoke.mockReturnValueOnce(inputsB.promise as any);
      mockedInvoke.mockReturnValueOnce(valuesB.promise as any);

      const mutation = useInputStore.getState().setValue('computer-a', 'token_a', 'new-a');
      const loadInputsB = useInputStore.getState().fetchInputs('computer-b');
      const loadValuesB = useInputStore.getState().fetchValues('computer-b');

      inputsB.resolve([{ type: 'PromptString', id: 'token_b', label: 'Token B' }]);
      valuesB.resolve({ token_b: { configured: true, status: 'configured', value: 'value-b' } });
      await loadInputsB;
      await loadValuesB;
      mutationA.resolve();
      await mutation;

      expect(mockedInvoke).toHaveBeenCalledTimes(3);
      expect(useInputStore.getState().activeInstanceId).toBe('computer-b');
      expect(useInputStore.getState().valuesLoadedInstanceId).toBe('computer-b');
      expect(useInputStore.getState().values).toEqual({
        token_b: { configured: true, status: 'configured', value: 'value-b' },
      });
    });

    it('does not surface a stale mutation failure on the active Computer', async () => {
      const mutationA = deferred<void>();
      useInputStore.setState({ activeInstanceId: 'computer-a' });
      mockedInvoke.mockReturnValueOnce(mutationA.promise as any);
      mockedInvoke.mockResolvedValueOnce([]);
      mockedInvoke.mockResolvedValueOnce({
        token_b: { configured: true, status: 'configured', value: 'value-b' },
      });

      const mutation = useInputStore.getState().setValue('computer-a', 'token_a', 'new-a');
      await useInputStore.getState().fetchInputs('computer-b');
      await useInputStore.getState().fetchValues('computer-b');
      mutationA.reject('Computer A keychain failed');

      await expect(mutation).rejects.toBe('Computer A keychain failed');
      expect(useInputStore.getState().activeInstanceId).toBe('computer-b');
      expect(useInputStore.getState().error).toBeNull();
      expect(useInputStore.getState().valuesError).toBeNull();
    });
  });

  describe('clearValues', () => {
    it('clears values map', async () => {
      useInputStore.setState({
        activeInstanceId: instanceId,
        valuesLoadedInstanceId: instanceId,
        values: {
          a: { configured: true, status: 'configured', value: '1' },
          b: { configured: true, status: 'configured', value: '2' },
        },
      });
      mockedInvoke.mockResolvedValueOnce(undefined);
      mockedInvoke.mockResolvedValueOnce({
        a: { configured: false, status: 'missing' },
        b: { configured: false, status: 'first_option' },
      });

      await useInputStore.getState().clearValues(instanceId);

      expect(mockedInvoke).toHaveBeenCalledWith('clear_input_values', { instanceId });
      expect(useInputStore.getState().values).toEqual({
        a: { configured: false, status: 'missing' },
        b: { configured: false, status: 'first_option' },
      });
    });
  });
});
