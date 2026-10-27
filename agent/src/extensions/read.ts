import { createReadTool } from '@earendil-works/pi-durable/tools';
import { untrustedPdf } from './pdf.ts';

export function createSnapshotReadTool() {
  const stock = createReadTool();
  return {
    ...stock,
    // Stock 1.4.0 read omits replay. Retain its implementation, but our snapshot
    // permits rerunning reads (D9). Never install CodingTools.
    replay: 'safe' as const,
    async execute(...args: Parameters<typeof stock.execute>) {
      const result = await stock.execute(...args);
      return { ...result, output: result.output?.map(item => item.type === 'text'
        ? { ...item, text: untrustedPdf(item.text) } : item) };
    },
  };
}
