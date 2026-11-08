// Builds the data files of the GitHub Pages site (site/data/*.json) from the local renders and committed assets.
//   bun tools/site/build.ts            → site/data/{songs,waves,stats}.json
// Inputs: build/gm/songs/{selection,results*}.json, build/gm/songs/waves/{gm,plus}/index.json (git-ignored renders),
// assets/{gm,gm-plus}/{scores.json,waves/} (committed). Audio is not copied: the pages play it from the bucket.
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { songTitle } from "./titles.ts";

const ROOT = join(import.meta.dir, "../..");
const SONGS = join(ROOT, "build/gm/songs");
const OUT = join(ROOT, "site/data");
export const AUDIO = "https://storage.googleapis.com/gm-procedural/gm/songs/";

const read = (p: string) => JSON.parse(readFileSync(p, "utf8"));
const r1 = (x: number | undefined) => (x === undefined || x === null || Number.isNaN(x) ? null : Math.round(x * 100) / 100);

type St = { tf: number; d_lufs: number; plus?: { tf: number; d_lufs: number }; sf?: { tf: number } };
const metrics = (s: St) => ({ tf: r1(s.tf), dl: r1(s.d_lufs), tfp: r1(s.plus?.tf), dlp: r1(s.plus?.d_lufs), tfs: r1(s.sf?.tf) });

export function songs() {
  const res: any[] = readdirSync(SONGS).filter((f) => /^results(\.\d+)?\.json$/.test(f)).flatMap((f) => { const j = read(join(SONGS, f)); return Array.isArray(j) ? j : Object.values(j); });
  const byId = new Map(res.map((r) => [r.id, r]));
  return [...byId.values()]
    .filter((r) => r.mix?.plus) // four-way renders only
    .map((r) => {
      const t = songTitle(r.id, r.title);
      return {
        id: r.id, src: r.src, title: t.title, by: t.by, seconds: r.seconds,
        mix: metrics(r.mix),
        stems: [...r.stems].sort((a: any, b: any) => b.notes - a.notes).map((s: any) => ({
          ch: s.ch, label: s.label.replace(/^ch\d+ /, "").replace(/^prog \d+ /, "").replace(/^Drums kit \d+ /, "Drums · "), drums: s.drums, notes: s.notes, waves: Object.keys(s.waves).map(Number), ...metrics(s),
        })),
      };
    })
    .sort((a, b) => (a.src === b.src ? a.title.localeCompare(b.title) : a.src === "novak" ? -1 : 1));
}

export function waves() {
  const g: any[] = read(join(SONGS, "waves/gm/index.json"));
  const p: any[] = read(join(SONGS, "waves/plus/index.json"));
  return g.map((w, i) => ({
    w: w.wave, cat: w.category, ins: w.instruments.map((s: string) => s.replace(/ \(bank (\d+) prog (\d+)\)/, (_, b, p) => ` (bank ${b}, program ${+p + 1})`).replace(/ \(kit\) keys (\d+)-\d+/, " kit, key $1")),
    g: r1(w.score * 100), p: r1(p[i]?.score * 100), src: p[i]?.source ?? null, fl: [...new Set([...(w.flags ?? []), ...(p[i]?.flags ?? [])])],
  }));
}

export function stats() {
  const set = (dir: string) => {
    const s = read(join(ROOT, `assets/${dir}/scores.json`));
    const v = Object.values(s).map((x: any) => x.score as number);
    const fam: Record<string, number> = {};
    for (const f of readdirSync(join(ROOT, `assets/${dir}/waves`))) for (const l of read(join(ROOT, `assets/${dir}/waves`, f)).layers ?? []) fam[l.family] = (fam[l.family] ?? 0) + 1;
    const sorted = [...v].sort((a, b) => a - b);
    return { n: v.length, A: v.filter((x) => x >= 0.85).length, B: v.filter((x) => x >= 0.7 && x < 0.85).length, C: v.filter((x) => x < 0.7).length, median: r1(sorted[sorted.length >> 1] * 100), fam, scores: s };
  };
  const gm = set("gm"), plus = set("gm-plus");
  const cats = read(join(SONGS, "waves/gm/index.json")) as any[];
  const byCat: Record<string, { n: number; gm: number; plus: number }> = {};
  for (const w of cats) {
    const c = (byCat[w.category] ??= { n: 0, gm: 0, plus: 0 });
    c.n++; c.gm += gm.scores[w.wave]?.score ?? 0; c.plus += plus.scores[w.wave]?.score ?? 0;
  }
  const categories = Object.entries(byCat).map(([k, c]) => ({ cat: k, n: c.n, gm: r1((c.gm / c.n) * 100), plus: r1((c.plus / c.n) * 100) })).sort((a, b) => b.gm! - a.gm!);
  const strip = ({ scores: _, ...x }: any) => x;
  return { gm: strip(gm), plus: strip(plus), categories };
}

if (import.meta.main) {
  if (!existsSync(join(SONGS, "waves/gm/index.json"))) throw new Error("no renders in build/gm/songs — run tools/gm-songs first");
  mkdirSync(OUT, { recursive: true });
  const s = songs(), w = waves(), st = stats();
  writeFileSync(join(OUT, "songs.json"), JSON.stringify({ audio: AUDIO, songs: s }));
  writeFileSync(join(OUT, "waves.json"), JSON.stringify({ audio: AUDIO + "waves/", waves: w }));
  writeFileSync(join(OUT, "stats.json"), JSON.stringify(st, null, 1));
  console.log(`site/data: ${s.length} songs, ${w.length} waves, gm A/B/C ${st.gm.A}/${st.gm.B}/${st.gm.C}`);
}
