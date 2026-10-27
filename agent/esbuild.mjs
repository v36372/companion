import { build } from 'esbuild';
await build({
  entryPoints: ['src/index.ts'], outfile: 'dist/guest.js',
  bundle: true, format: 'esm', platform: 'neutral', target: 'es2022',
  external: ['wasi:*', 'companion:agent/*'],
});
