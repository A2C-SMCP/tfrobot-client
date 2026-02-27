import { invoke } from '@tauri-apps/api/core';
import { useInputStore, type InputDefinition } from '@/stores/inputStore';

const mockedInvoke = vi.mocked(invoke);

function resetStore() {
  useInputStore.setState({ inputs: [], values: {}, loading: false, error: null });
}

describe('inputStore', () => {
  beforeEach(() => {
    resetStore();
    mockedInvoke.mockReset();
  });

  describe('fetchInputs', () => {
    it('populates inputs list', async () => {
      const mockInputs: InputDefinition[] = [
        { type: 'PromptString', id: 'api_key', label: 'API Key' },
      ];
      mockedInvoke.mockResolvedValueOnce(mockInputs);

      await useInputStore.getState().fetchInputs();

      expect(mockedInvoke).toHaveBeenCalledWith('list_inputs');
      expect(useInputStore.getState().inputs).toEqual(mockInputs);
      expect(useInputStore.getState().loading).toBe(false);
    });

    it('sets error on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('load failed');

      await useInputStore.getState().fetchInputs();

      expect(useInputStore.getState().error).toBe('load failed');
      expect(useInputStore.getState().loading).toBe(false);
    });
  });

  describe('fetchValues', () => {
    it('populates values map', async () => {
      const mockValues = { api_key: 'secret123' };
      mockedInvoke.mockResolvedValueOnce(mockValues);

      await useInputStore.getState().fetchValues();

      expect(mockedInvoke).toHaveBeenCalledWith('list_input_values');
      expect(useInputStore.getState().values).toEqual(mockValues);
    });
  });

  describe('addOrUpdateInput', () => {
    it('invokes add_or_update_input and refreshes', async () => {
      const input: InputDefinition = { type: 'PromptString', id: 'token', label: 'Token' };
      mockedInvoke.mockResolvedValueOnce(undefined); // add_or_update_input
      mockedInvoke.mockResolvedValueOnce([input]);   // fetchInputs

      await useInputStore.getState().addOrUpdateInput(input);

      expect(mockedInvoke).toHaveBeenCalledWith('add_or_update_input', { input });
      expect(useInputStore.getState().inputs).toEqual([input]);
    });

    it('sets error and re-throws on failure', async () => {
      mockedInvoke.mockRejectedValueOnce('save failed');

      await expect(
        useInputStore.getState().addOrUpdateInput({ type: 'PromptString', id: 'x', label: 'X' })
      ).rejects.toBe('save failed');

      expect(useInputStore.getState().error).toBe('save failed');
    });
  });

  describe('removeInput', () => {
    it('invokes remove_input and refreshes inputs + values', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined); // remove_input
      mockedInvoke.mockResolvedValueOnce([]);         // fetchInputs
      mockedInvoke.mockResolvedValueOnce({});          // fetchValues

      await useInputStore.getState().removeInput('api_key');

      expect(mockedInvoke).toHaveBeenCalledWith('remove_input', { id: 'api_key' });
    });
  });

  describe('setValue', () => {
    it('invokes set_input_value and refreshes values', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);     // set_input_value
      mockedInvoke.mockResolvedValueOnce({ key: 'val' }); // fetchValues

      await useInputStore.getState().setValue('key', 'val');

      expect(mockedInvoke).toHaveBeenCalledWith('set_input_value', { id: 'key', value: 'val' });
    });
  });

  describe('clearValues', () => {
    it('clears values map', async () => {
      useInputStore.setState({ values: { a: '1', b: '2' } });
      mockedInvoke.mockResolvedValueOnce(undefined);

      await useInputStore.getState().clearValues();

      expect(mockedInvoke).toHaveBeenCalledWith('clear_input_values');
      expect(useInputStore.getState().values).toEqual({});
    });
  });
});
