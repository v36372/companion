declare module 'companion:agent/host-events@0.1.0' {
  export function onCommit(json: string): void;
  export function onStream(json: string): void;
  export function onReport(json: string): void;
}
