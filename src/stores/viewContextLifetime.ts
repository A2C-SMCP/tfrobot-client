// Shared UI stores belong to the current application identity. A navigation reset
// must also invalidate continuations of mutations that started in the old identity.
let generation = 0;

export function captureViewContext(): () => boolean {
  const started = generation;
  return () => generation === started;
}

export function retireViewContext(): void { generation += 1; }
