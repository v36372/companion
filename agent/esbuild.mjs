import { build } from 'esbuild';
import { mkdir, writeFile } from 'node:fs/promises';
import { tokenizer } from 'acorn';
import rewritePattern from 'regexpu-core';
const result = await build({
  entryPoints: ['src/component.ts'], outfile: 'dist/guest.js',
  bundle: true, format: 'esm', platform: 'browser', target: 'es2022',
  external: ['wasi:*', 'companion:agent/*'],
  write: false,
});
// StarlingMonkey's small ICU lacks Unicode property escapes. Lower their exact
// Unicode sets at build time (not an ASCII approximation or runtime polyfill).
let code = result.outputFiles[0].text;
const replacements = [];
for (const token of tokenizer(code, { ecmaVersion: 'latest', sourceType: 'module' })) {
  if (token.type.label === 'regexp' && /\\[pP]\{/.test(token.value.pattern)) {
    const { pattern, flags } = token.value;
    const lowered = rewritePattern(pattern, flags, { unicodePropertyEscapes: 'transform', unicodeFlag: 'transform' });
    replacements.push([token.start, token.end, `/${lowered}/${flags.replace('u', '')}`]);
  }
}
for (const [start, end, value] of replacements.reverse()) code = code.slice(0, start) + value + code.slice(end);
await mkdir('dist', { recursive: true });
await writeFile('dist/guest.js', code);
