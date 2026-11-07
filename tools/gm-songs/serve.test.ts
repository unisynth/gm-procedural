import { afterAll, expect, test } from "bun:test";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { serve } from "./serve";

const dir = mkdtempSync(join(tmpdir(), "gm-serve-"));
writeFileSync(join(dir, "a.mp3"), new Uint8Array(4000).map((_, i) => i & 255));
writeFileSync(join(dir, "index.html"), "<p>hi</p>");
const s = serve(dir, 0), url = `http://localhost:${s.port}`;
afterAll(() => s.stop(true));

test("Range requests get 206 and the slice", async () => {
  const r = await fetch(`${url}/a.mp3`, { headers: { Range: "bytes=1000-1999" } });
  expect(r.status).toBe(206);
  const b = new Uint8Array(await r.arrayBuffer());
  expect(b.length).toBe(1000);
  expect(b[0]).toBe(1000 & 255);
});

test("index.html for a directory, 404 outside the root", async () => {
  expect(await (await fetch(`${url}/`)).text()).toBe("<p>hi</p>");
  expect((await fetch(`${url}/..%2f..%2fetc%2fpasswd`)).status).toBe(404);
});
