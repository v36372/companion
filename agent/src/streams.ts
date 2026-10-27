// StarlingMonkey bodies have getReader but lack async iteration (PR4).
const originalFetch = globalThis.fetch;
globalThis.fetch = async (...args: Parameters<typeof fetch>) => {
  const response = await originalFetch(...args);
  const body = response.body;
  if (body && !body[Symbol.asyncIterator]) {
    Object.defineProperty(body, Symbol.asyncIterator, {
      value: async function* () {
        const reader = this.getReader();
        try {
          for (;;) {
            const { done, value } = await reader.read();
            if (done) return;
            yield value;
          }
        } finally { reader.releaseLock(); }
      },
    });
  }
  return response;
};
