// Outside Node.js (browsers, Cloudflare Workers) there is no `nyra` executable to start.
export const available = false;
export function findBinary() {
  return null;
}
export class NativeBackend {
  constructor() {
    throw new Error("the native backend needs Node.js (or Deno, Bun); use the WebAssembly backend here");
  }
}
