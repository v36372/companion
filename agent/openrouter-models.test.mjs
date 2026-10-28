import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createModels } from '@earendil-works/pi-ai/models';
import { openrouterProvider } from '@earendil-works/pi-ai/providers/openrouter';
import { selectableOpenrouterProvider } from './src/openrouter-models.ts';

test('custom OpenRouter slugs resolve locally and known metadata stays intact', () => {
  const overlay = selectableOpenrouterProvider();
  const models = createModels();
  models.setProvider(overlay.provider);
  const known = openrouterProvider().getModels().find(model => model.id === 'openai/gpt-4o-mini');
  assert.ok(known);
  overlay.ensure(known.id);
  assert.deepEqual(models.getModel('openrouter', known.id), known);
  const slug = 'vendor/new-model:v1';
  assert.equal(models.getModel('openrouter', slug), undefined);
  overlay.ensure(slug);
  overlay.ensure(slug);
  const custom = models.getModel('openrouter', slug);
  assert.equal(custom.id, slug);
  assert.equal(custom.api, 'openai-completions');
  assert.deepEqual(custom.input, ['text']);
  assert.equal(custom.contextWindow, 32768);
  assert.equal(custom.maxTokens, 4096);
  assert.deepEqual(custom.cost, { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 });
  assert.equal(models.getModels().filter(model => model.id === slug).length, 1);
  assert.deepEqual(models.getModel('openrouter', known.id), known);
  assert.throws(() => overlay.ensure('invalid model'));
});
