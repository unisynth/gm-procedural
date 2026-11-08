// Runs gmgen.wasm (crates/gmgen-wasm) under bun, exactly as site/assets/gmgen-worker.js does in the browser:
// begin(set) → step(i) for every wave → finish() → DLS bytes at out_ptr(). Used by build.ts to make the prebuilt
// gen-gm bank the site downloads by default (so it is the wasm's own output, byte for byte).
import { readFileSync } from "node:fs";

export type GmgenSet = 0 | 1; // 0 = gen-gm, 1 = gen-plus

export async function wasmBank(wasmPath: string, set: GmgenSet): Promise<Uint8Array> {
  const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
  const w = instance.exports as any;
  const n = w.begin(set);
  for (let i = 0; i < n; i++) w.step(i);
  const len = w.finish();
  const out = new Uint8Array(w.memory.buffer, w.out_ptr(), len).slice();
  w.release();
  return out;
}

export const sha1 = (b: Uint8Array) => new Bun.CryptoHasher("sha1").update(b).digest("hex");
