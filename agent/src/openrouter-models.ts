import type { Model } from '@earendil-works/pi-ai';
import { openrouterProvider } from '@earendil-works/pi-ai/providers/openrouter';

/** Local overlay only: never fetch a catalog or silently substitute a slug. */
export function selectableOpenrouterProvider() {
  const provider = openrouterProvider();
  const known = provider.getModels();
  const custom = new Map<string, Model<'openai-completions'>>();
  return {
    provider: { ...provider, getModels: () => [...known, ...custom.values()] },
    ensure(modelId: string) {
      if (!/^[A-Za-z0-9~._:/-]{1,128}$/.test(modelId)) throw new Error('invalid model id');
      if (known.some(model => model.id === modelId) || custom.has(modelId)) return;
      // Unknown models have no trustworthy limits/pricing metadata. Use small,
      // text-only limits and do not claim estimated spend reflects actual bills.
      custom.set(modelId, {
        id: modelId, name: modelId, provider: 'openrouter',
        api: 'openai-completions', baseUrl: 'https://openrouter.ai/api/v1',
        reasoning: false, input: ['text'], contextWindow: 32768, maxTokens: 4096,
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
        compat: { supportsDeveloperRole: false, thinkingFormat: 'openrouter' },
      });
    },
  };
}
