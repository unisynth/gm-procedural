// Static server for the comparison pages. `Bun.file` responses answer Range requests with 206, which the players
// need to seek and to switch banks at the same position (a server answering 200 to Range breaks both in Chrome).
import { existsSync, statSync } from "node:fs";
import { resolve } from "node:path";

export function serve(root: string, port: number) {
  const base = resolve(root);
  return Bun.serve({
    port,
    fetch(req) {
      const rel = decodeURIComponent(new URL(req.url).pathname).replace(/\/$/, "/index.html");
      const path = resolve(base, "." + rel);
      if (!path.startsWith(base + "/") || !existsSync(path) || !statSync(path).isFile()) return new Response("not found", { status: 404 });
      return new Response(Bun.file(path));
    },
  });
}
