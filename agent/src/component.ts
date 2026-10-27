// WIT result errors must lower as strings, never Error/DOMException objects.
import './abort-signals.ts';
import * as impl from './index.ts';
function lower<A extends unknown[], R>(fn: (...args: A) => Promise<R>) {
  return async (...args: A): Promise<R> => {
    try { return await fn(...args); } catch (error) { throw String(error); }
  };
}
export const open = lower(impl.open);
export const openDocument = lower(impl.openDocument);
export const submit = lower(impl.submit);
export const resumeWait = lower(impl.resumeWait);
// WIT s64 arrives as bigint; Durable's branded ids are safe JS numbers.
export const abort = lower(async (id: bigint) => impl.abort(Number(id)));
export const configure = lower(async (id: bigint, json: string) => impl.configure(Number(id), json));
export const setFollow = lower(async (id: bigint, enabled: boolean) => impl.setFollow(Number(id), enabled));
export const close = impl.close;
export const spikeSse = impl.spikeSse;
export const spikeBusy = impl.spikeBusy;
