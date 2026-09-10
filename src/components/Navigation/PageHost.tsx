import { App, ConfigProvider } from 'antd';
import { useCallback, useContext, useLayoutEffect, useRef, useState, type PropsWithChildren } from 'react';
import { ScopeContext } from './navigationMemoryState';
import { PageActivity } from './PageActivity';
import { usePageActive } from './pageActivityState';

/** Fixed navigation slots are initialized on first visit and owned until scope disposal. */
export function PageHost({ active, children, name }: PropsWithChildren<{ active: boolean; name: string }>) {
  const scope = useContext(ScopeContext);
  const parentActive = usePageActive();
  const visible = active && parentActive;
  const [visited, setVisited] = useState(visible);
  const host = useRef<HTMLDivElement>(null);
  const innerScroll = useRef(new Map<Element, [number, number]>());
  const savedPanes = useRef<Record<string, [number, number]>>(scope?.get(`scroll-panes.${name}`) as Record<string, [number, number]> ?? {});
  const scroll = useRef<[number, number]>(scope?.get(`scroll.${name}`) as [number, number] ?? [0, 0]);
  if (visible && !visited) setVisited(true);
  const popupContainer = useCallback(() => host.current ?? document.body, []);

  useLayoutEffect(() => {
    if (!visible) return;
    const [left, top] = scroll.current;
    const ownsWindow = () => !host.current?.querySelector('[data-page]:not([hidden])');
    let windowPending = ownsWindow();
    for (const [node, [x, y]] of innerScroll.current) {
      if (node.isConnected && node.closest('[data-page]') === host.current) { node.scrollLeft = x; node.scrollTop = y; }
      else innerScroll.current.delete(node);
    }
    // Named regions survive object-tree disposal without retaining DOM nodes.
    // Data can arrive after mount, so restore each region once it can scroll to
    // its saved position; subsequent updates must not move the user's cursor.
    const pending = new Map(Object.entries(savedPanes.current));
    let observer: MutationObserver | null = null;
    let resize: ResizeObserver | null = null;
    const restorePanes = () => {
      if (windowPending && !ownsWindow()) windowPending = false;
      if (windowPending) {
        const root = document.documentElement;
        if (Math.max(0, root.scrollHeight - window.innerHeight) >= top
          && Math.max(0, root.scrollWidth - window.innerWidth) >= left) {
          window.scrollTo?.({ left, top, behavior: 'instant' });
          windowPending = false;
        }
      }
      for (const node of host.current?.querySelectorAll<HTMLElement>('[data-navigation-scroll]') ?? []) {
        if (node.closest('[data-page]') !== host.current) continue;
        const id = node.dataset.navigationScroll!;
        const position = pending.get(id);
        if (!position) continue;
        const [x, y] = position;
        if (node.scrollWidth - node.clientWidth < x || node.scrollHeight - node.clientHeight < y) continue;
        node.scrollLeft = x;
        node.scrollTop = y;
        pending.delete(id);
      }
      if (pending.size === 0 && !windowPending) { observer?.disconnect(); resize?.disconnect(); }
    };
    restorePanes();
    if ((pending.size > 0 || windowPending) && host.current) {
      observer = new MutationObserver(restorePanes);
      observer.observe(host.current, { childList: true, subtree: true, characterData: true });
      resize = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(restorePanes);
      resize?.observe(host.current);
    }
    const save = () => {
      if (!ownsWindow() || windowPending) return;
      scroll.current = [window.scrollX, window.scrollY];
      scope?.set(`scroll.${name}`, scroll.current);
    };
    window.addEventListener('scroll', save, { passive: true });
    return () => { window.removeEventListener('scroll', save); observer?.disconnect(); resize?.disconnect(); };
  }, [name, scope, visible]);

  return (
    <div ref={host} onScrollCapture={(event) => {
      if (visible && event.target instanceof Element && event.target.closest('[data-page]') === host.current) {
        const position: [number, number] = [event.target.scrollLeft, event.target.scrollTop];
        innerScroll.current.set(event.target, position);
        const id = event.target.getAttribute('data-navigation-scroll');
        if (id) {
          savedPanes.current = { ...savedPanes.current, [id]: position };
          scope?.set(`scroll-panes.${name}`, savedPanes.current);
        }
      }
    }} hidden={!visible} aria-hidden={!visible} data-page={name} style={{ minWidth: 0 }}>
      {visited && (
        <PageActivity active={active}>
          <ConfigProvider getPopupContainer={popupContainer}>
            <App component={false}>{children}</App>
          </ConfigProvider>
        </PageActivity>
      )}
    </div>
  );
}
