import { useEffect, useState } from 'react';
import { Button, Form, Input } from 'antd';
import { act, fireEvent, render, screen } from '../helpers/render';
import { PageHost } from '@/components/Navigation/PageHost';
import { PageModal, PagePopconfirm } from '@/components/Navigation/PageOverlays';
import { NavigationMemoryProvider, NavigationScope } from '@/components/Navigation/NavigationMemory';
import { useNavigationForm, useNavigationState } from '@/components/Navigation/navigationMemoryState';
import { createNavigationStore, NavigationMemory } from '@/stores/navigationStore';

describe('navigation ownership', () => {
  it('restores menu destinations and distinguishes repeated explicit navigation', () => {
    const store = createNavigationStore();
    store.getState().navigate('computer-settings:inputs');
    store.getState().openMenu('chat');
    store.getState().openMenu('computer');
    expect(store.getState().route).toBe('computer-settings:inputs');
    expect(store.getState().revision).toBe(1);
    store.getState().navigate('computer-settings:inputs');
    expect(store.getState().revision).toBe(2);
  });

  it('initializes lazily, retains input and receives events while hidden with one subscription', () => {
    const events = new EventTarget();
    const mounted = vi.fn();
    const disposed = vi.fn();
    function Page() {
      const [draft, setDraft] = useState('');
      const [count, setCount] = useState(0);
      useEffect(() => {
        mounted();
        const receive = () => setCount((value) => value + 1);
        events.addEventListener('message', receive);
        return () => { disposed(); events.removeEventListener('message', receive); };
      }, []);
      return <><input aria-label="draft" value={draft} onChange={(event) => setDraft(event.target.value)} /><output>{count}</output></>;
    }
    const view = render(<PageHost name="test" active={false}><Page /></PageHost>);
    expect(mounted).not.toHaveBeenCalled();
    view.rerender(<PageHost name="test" active><Page /></PageHost>);
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'unfinished' } });
    for (let i = 0; i < 5; i++) {
      view.rerender(<PageHost name="test" active={false}><Page /></PageHost>);
      act(() => events.dispatchEvent(new Event('message')));
      view.rerender(<PageHost name="test" active><Page /></PageHost>);
    }
    expect(screen.getByRole('textbox')).toHaveValue('unfinished');
    expect(screen.getByRole('status')).toHaveTextContent('5');
    expect(mounted).toHaveBeenCalledOnce();
    expect(disposed).not.toHaveBeenCalled();
    view.unmount();
    expect(disposed).toHaveBeenCalledOnce();
  });

  it('restores per-object drafts, protects edits from refresh, and removes deleted-object state', () => {
    const memory = new NavigationMemory();
    function Editor({ name }: { name: string }) {
      const [form] = Form.useForm<{ name: string; description: string }>();
      const draft = useNavigationForm('form', form, { name, description: name });
      const [filter, setFilter] = useNavigationState('filter', '');
      return <><Form form={form} onValuesChange={draft.onValuesChange}>
        <Form.Item name="name"><Input aria-label="name" /></Form.Item>
        <Form.Item name="description"><Input aria-label="description" /></Form.Item>
      </Form><Input aria-label="filter" value={filter} onChange={(event) => setFilter(event.target.value)} /></>;
    }
    const tree = (id: string, name: string) => <NavigationMemoryProvider memory={memory}>
      <NavigationScope id={`computer:${id}`} key={id}><Editor name={name} /></NavigationScope>
    </NavigationMemoryProvider>;
    const view = render(tree('a', 'A'));
    fireEvent.change(screen.getByLabelText('name'), { target: { value: 'draft A' } });
    fireEvent.change(screen.getByLabelText('filter'), { target: { value: 'tools' } });
    view.rerender(tree('a', 'refreshed A'));
    expect(screen.getByLabelText('name')).toHaveValue('draft A');
    expect(screen.getByLabelText('description')).toHaveValue('refreshed A');
    view.rerender(tree('b', 'B'));
    expect(screen.getByLabelText('name')).toHaveValue('B');
    view.rerender(tree('a', 'refreshed A'));
    expect(screen.getByLabelText('name')).toHaveValue('draft A');
    expect(screen.getByLabelText('filter')).toHaveValue('tools');
    view.rerender(tree('b', 'B'));
    memory.pruneComputers(new Set(['b']));
    view.rerender(tree('a', 'new A'));
    expect(screen.getByLabelText('name')).toHaveValue('new A');
  });

  it('merges nested dirty fields while allowing untouched siblings to refresh', () => {
    const memory = new NavigationMemory();
    function Editor({ baseline }: { baseline: string | undefined }) {
      const [form] = Form.useForm();
      const draft = useNavigationForm('nested', form, { metadata: { first: 'first', second: 'second', fresh: baseline } });
      return <Form form={form} onValuesChange={draft.onValuesChange}>
        <Form.Item name={['metadata', 'first']}><Input aria-label="first" /></Form.Item>
        <Form.Item name={['metadata', 'second']}><Input aria-label="second" /></Form.Item>
        <Form.Item name={['metadata', 'fresh']}><Input aria-label="fresh" /></Form.Item>
      </Form>;
    }
    const tree = (id: string, baseline: string | undefined) => <NavigationMemoryProvider memory={memory}>
      <NavigationScope id={id} key={id}><Editor baseline={baseline} /></NavigationScope>
    </NavigationMemoryProvider>;
    const view = render(tree('a', 'old'));
    fireEvent.change(screen.getByLabelText('first'), { target: { value: 'edited first' } });
    fireEvent.change(screen.getByLabelText('second'), { target: { value: 'edited second' } });
    view.rerender(tree('b', 'old'));
    view.rerender(tree('a', 'refreshed'));
    expect(screen.getByLabelText('first')).toHaveValue('edited first');
    expect(screen.getByLabelText('second')).toHaveValue('edited second');
    expect(screen.getByLabelText('fresh')).toHaveValue('refreshed');
    view.rerender(tree('a', undefined));
    expect(screen.getByLabelText('fresh')).toHaveValue('');
    expect(screen.getByLabelText('first')).toHaveValue('edited first');
  });

  it('restores named overflow panes after the Computer subtree is released', () => {
    const memory = new NavigationMemory();
    const pane = (node: HTMLDivElement | null) => {
      if (node) Object.defineProperties(node, {
        scrollHeight: { configurable: true, value: 1000 },
        clientHeight: { configurable: true, value: 100 },
      });
    };
    const tree = (id: string) => <NavigationMemoryProvider memory={memory}>
      <NavigationScope id={`computer:${id}`} key={id}>
        <PageHost name="details" active><div ref={pane} data-testid="pane" data-navigation-scroll="tools" style={{ overflow: 'auto', height: 100 }}>
          <div style={{ height: 1000 }}>Tools</div>
        </div></PageHost>
      </NavigationScope>
    </NavigationMemoryProvider>;
    const view = render(tree('a'));
    const original = screen.getByTestId('pane');
    original.scrollTop = 350;
    fireEvent.scroll(original);
    view.rerender(tree('b'));
    expect(screen.getByTestId('pane').scrollTop).toBe(0);
    view.rerender(tree('a'));
    expect(screen.getByTestId('pane')).not.toBe(original);
    expect(screen.getByTestId('pane').scrollTop).toBe(350);
  });

  it('hides editors without discarding drafts and cancels dangerous confirmation on navigation', () => {
    const confirm = vi.fn();
    function Editor() {
      return <><PageModal open footer={null} title="Editor"><Input aria-label="modal draft" /></PageModal>
        <PagePopconfirm title="Delete permanently?" onConfirm={confirm}><Button>Delete</Button></PagePopconfirm></>;
    }
    const tree = (active: boolean) => <PageHost name="editor" active={active}><Editor /></PageHost>;
    const view = render(tree(true));
    fireEvent.change(screen.getByLabelText('modal draft'), { target: { value: 'draft' } });
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    view.rerender(tree(false));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    view.rerender(tree(true));
    expect(screen.getByLabelText('modal draft')).toHaveValue('draft');
    expect(screen.queryByRole('button', { name: 'OK' })).not.toBeInTheDocument();
    expect(confirm).not.toHaveBeenCalled();
  });
});
