import { createStore } from 'zustand/vanilla';

export function menuForRoute(route: string): string {
  return route.startsWith('computer') ? 'computer' : route;
}

interface NavigationState {
  route: string;
  destinations: Record<string, string>;
  revision: number;
  requests: Record<string, { route: string; revision: number }>;
  navigate: (route: string) => void;
  openMenu: (menu: string) => void;
}

/** One store per application identity lifetime. Nothing is persisted to disk. */
export function createNavigationStore() {
  return createStore<NavigationState>((set, get) => ({
    route: 'chat',
    destinations: {},
    revision: 0,
    requests: {},
    navigate: (route) => set((state) => ({
      route,
      requests: { ...state.requests, [route.split(':')[0]]: { route, revision: state.revision + 1 } },
      destinations: { ...state.destinations, [menuForRoute(route)]: route },
      revision: state.revision + 1,
    })),
    openMenu: (menu) => set({ route: get().destinations[menu] ?? menu }),
  }));
}

/** Entries belong to existing objects only. A retired scope rejects late writes. */
export class NavigationMemory {
  private scopes = new Map<string, Map<string, unknown>>();
  scope(id: string) {
    let values = this.scopes.get(id);
    if (!values) {
      values = new Map();
      this.scopes.set(id, values);
    }
    return values;
  }
  pruneComputers(ids: ReadonlySet<string>) {
    for (const key of this.scopes.keys()) {
      if (key.startsWith('computer:') && !ids.has(key.slice('computer:'.length))) {
        this.scopes.delete(key);
      }
    }
  }
  clear() { this.scopes.clear(); }
}
