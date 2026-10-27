// StarlingMonkey 0.22's AbortSignal.any can return an already-aborted signal
// (reason 5e-324) for two fresh signals. The OpenAI SDK composes them before
// fetch, so patch composition, NOT fetch or the SDK. Keep native signals and
// cancellation; host HTTP abort still wakes the WASI pollable (D11).
export function anySignal(signals: Iterable<AbortSignal>): AbortSignal {
  const inputs = Array.from(signals);
  for (const signal of inputs) {
    if (!(signal instanceof AbortSignal)) throw new TypeError('expected AbortSignal');
  }
  const controller = new AbortController();
  const cleanup: (() => void)[] = [];
  const forward = (signal: AbortSignal) => {
    if (!controller.signal.aborted) controller.abort(signal.reason);
    for (const remove of cleanup.splice(0)) remove();
  };
  for (const signal of inputs) {
    if (signal.aborted) { forward(signal); break; }
    const listener = () => forward(signal);
    signal.addEventListener('abort', listener, { once: true });
    cleanup.push(() => signal.removeEventListener('abort', listener));
  }
  return controller.signal;
}
AbortSignal.any = anySignal;
