import { onStream } from 'companion:agent/host-events@0.1.0';

/** PR4: the Promise and stream stay on-stack until EOF/error. No poll export. */
export async function spikeSse(url: string): Promise<string> {
  try {
    const response = await fetch(url);
    if (!response.ok || !response.body) throw new Error(`HTTP ${response.status}`);
    // StarlingMonkey 0.22.0 exposes getReader(), but not the standard stream
    // async iterator. Adapt reads directly: no collection, batching or timers.
    if (!response.body[Symbol.asyncIterator]) {
      Object.defineProperty(response.body, Symbol.asyncIterator, {
        value: async function* () {
          const reader = this.getReader();
          try {
            for (;;) {
              const { done, value } = await reader.read();
              if (done) return;
              yield value;
            }
          } finally {
            reader.releaseLock();
          }
        },
      });
    }
    let sequence = 0;
    let bytes = 0;
    for await (const chunk of response.body) {
      bytes += chunk.byteLength;
      onStream(JSON.stringify({ sequence: sequence++, bytes: chunk.byteLength }));
    }
    return JSON.stringify({ chunks: sequence, bytes });
  } catch (error) {
    // WIT result<_, string> requires a STRING payload, not Error/DOMException.
    throw String(error);
  }
}

export function spikeBusy(): never { for (;;) {} }

// Production interfaces remain present, but cannot accidentally pretend to run
// a conversation before PR5 provides storage, credentials and the harness.
function unavailable(): never { throw 'PR4 spike: harness not installed'; }
export const open = unavailable;
export const openDocument = unavailable;
export const submit = unavailable;
export const resumeWait = unavailable;
export const abort = unavailable;
export const setFollow = unavailable;
export const configure = unavailable;
export function close(): void {}
