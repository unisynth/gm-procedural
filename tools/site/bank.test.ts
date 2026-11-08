import { expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { sha1, wasmBank } from "./bank";

const ROOT = join(import.meta.dir, "../..");
const WASM = join(ROOT, "target/wasm32-unknown-unknown/fast/gmgen_wasm.wasm");
const NATIVE = join(ROOT, "build/gm/gen.dls");

// The prebuilt gen-gm the site downloads must be what the browser would build: wasm output == native `gmfit bank`.
test.skipIf(!existsSync(WASM) || !existsSync(NATIVE))("wasm gen-gm equals native build/gm/gen.dls", async () => {
  const b = await wasmBank(WASM, 0);
  expect(b.length).toBe(readFileSync(NATIVE).length);
  expect(sha1(b)).toBe(sha1(readFileSync(NATIVE)));
}, 60_000);
