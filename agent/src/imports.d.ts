declare module 'companion:agent/sqlite@0.1.0' {
  export function exec(sql: string): void;
  export function run(sql: string, params: import('./storage.ts').WireValue[]): void;
  export function get(sql: string, params: import('./storage.ts').WireValue[]): string | undefined;
  export function all(sql: string, params: import('./storage.ts').WireValue[]): string;
  export function begin(): void;
  export function commit(): void;
  export function rollback(): void;
}
declare module 'companion:agent/host-events@0.1.0' {
  export function onCommit(json: string): void;
  export function onStream(json: string): void;
  export function onReport(json: string): void;
}
