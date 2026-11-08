// Builds the GitHub Pages site's generated parts:
//   bun tools/site/build.ts   → site/data/{songs,waves,stats}.json, site/assets/{live.js, spessasynth_processor.min.js, gmgen.wasm}
// Inputs: build/site-audio/{results*.json, waves.json} (tools/spessa/render.ts), build/gm/songs/selection.json,
// build/gm/songs/waves/{gm,plus}/index.json (`gmfit dump`, scores and flags), assets/ (committed), and
// target/wasm32-unknown-unknown/fast/gmgen_wasm.wasm (`cargo build --profile fast -p gmgen-wasm --target wasm32-unknown-unknown`).
// Audio is not copied: gm.dls renders, MIDI and gmplus.sf2 stream from the bucket (tools/publish.sh).
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { songTitle } from "./titles.ts";

const ROOT = join(import.meta.dir, "../..");
const AUDIO = join(ROOT, "build/site-audio");
const SONGS = join(ROOT, "build/gm/songs");
const OUT = join(ROOT, "site/data");
export const BUCKET = "https://storage.googleapis.com/gm-procedural/site/";

const read = (p: string) => JSON.parse(readFileSync(p, "utf8"));
const r1 = (x: number | undefined | null) => (x === undefined || x === null || Number.isNaN(x) ? null : Math.round(x * 100) / 100);

type Ins = { drums: boolean; bank: number[]; program: number; name: string; regions: { key: number[]; wave: number }[] };
const structure = () => read(join(ROOT, "assets/gm/structure.json")) as { instruments: Ins[] };

/** Instrument a channel part plays: bank 0 melodic program, or the kit (drums, program) — names trimmed. */
function instrument(ins: Ins[], drums: boolean, program: number) {
  return ins.find((i) => i.drums === drums && i.bank[0] === 0 && i.program === program) ?? (drums ? ins.find((i) => i.drums) : undefined);
}
type Part = { ch: number; drums: boolean; notes: number; programs: Record<string, number>; keys: Record<string, number> };
function partInfo(ins: Ins[], p: Part) {
  const progs = Object.entries(p.programs).sort((a, b) => b[1] - a[1]);
  const main = +(progs[0]?.[0] ?? 0);
  const i = instrument(ins, p.drums, main);
  const name = i?.name.trim() ?? `program ${main + 1}`;
  const waves = new Set<number>();
  for (const [prog] of progs) {
    const it = instrument(ins, p.drums, +prog);
    for (const k of Object.keys(p.keys).map(Number)) for (const r of it?.regions ?? []) if (k >= r.key[0] && k <= r.key[1]) waves.add(r.wave);
  }
  return { label: p.drums ? `Drums · ${name}` : name + (progs.length > 1 ? ` +${progs.length - 1}` : ""), waves: [...waves].sort((a, b) => a - b) };
}

const m = (x: any) => ({ tf: r1(x.tf), dl: r1(x.d_lufs), tfp: r1(x.plus?.tf), dlp: r1(x.plus?.d_lufs), tfs: r1(x.sf?.tf) });

export function songs() {
  const ins = structure().instruments;
  const sel = new Map((read(join(SONGS, "selection.json")) as { id: string; src: string; title: string; parts: Part[] }[]).map((s) => [s.id, s]));
  const res: any[] = readdirSync(AUDIO).filter((f) => /^results(\.\d+)?\.json$/.test(f)).flatMap((f) => Object.values(read(join(AUDIO, f))));
  return res.filter((r) => sel.has(r.id)).map((r) => {
    const s = sel.get(r.id)!, t = songTitle(r.id, s.title);
    const parts = new Map(s.parts.map((p) => [p.ch, p]));
    return {
      id: r.id, src: s.src, title: t.title, by: t.by, seconds: r.seconds, mix: m(r.mix),
      stems: r.stems.map((x: any) => { const p = parts.get(x.ch)!; return { ch: x.ch, drums: p.drums, notes: p.notes, ...partInfo(ins, p), ...m(x) }; })
        .sort((a: any, b: any) => b.notes - a.notes),
    };
  }).sort((a, b) => (a.src === b.src ? a.title.localeCompare(b.title) : a.src === "novak" ? -1 : 1));
}

export function waves() {
  const g: any[] = read(join(SONGS, "waves/gm/index.json"));
  const p: any[] = read(join(SONGS, "waves/plus/index.json"));
  const notes: any[] = read(join(AUDIO, "waves.json"));
  return g.map((w, i) => {
    const n = notes[i];
    return {
      w: w.wave, cat: w.category, ins: w.instruments.map((s: string) => s.replace(/ \(bank (\d+) prog (\d+)\)/, (_, b, q) => ` (bank ${b}, program ${+q + 1})`).replace(/ \(kit\) keys (\d+)-\d+/, " kit, key $1")),
      g: r1(w.score * 100), p: r1(p[i]?.score * 100), src: p[i]?.source ?? null, fl: [...new Set([...(w.flags ?? []), ...(p[i]?.flags ?? [])])],
      note: n ? { drums: n.drums, bank: n.bank, program: n.program, key: n.key } : null,
    };
  });
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
  const byCat: Record<string, { n: number; gm: number; plus: number }> = {};
  for (const w of read(join(SONGS, "waves/gm/index.json")) as any[]) {
    const c = (byCat[w.category] ??= { n: 0, gm: 0, plus: 0 });
    c.n++; c.gm += gm.scores[w.wave]?.score ?? 0; c.plus += plus.scores[w.wave]?.score ?? 0;
  }
  const categories = Object.entries(byCat).map(([k, c]) => ({ cat: k, n: c.n, gm: r1((c.gm / c.n) * 100), plus: r1((c.plus / c.n) * 100) })).sort((a, b) => b.gm! - a.gm!);
  const strip = ({ scores: _, ...x }: any) => x;
  return { gm: strip(gm), plus: strip(plus), categories };
}

async function assets() {
  const dst = join(ROOT, "site/assets");
  const r = await Bun.build({ entrypoints: [join(import.meta.dir, "live.ts")], outdir: dst, target: "browser", format: "esm", minify: true, naming: "live.js" });
  if (!r.success) throw new Error(r.logs.join("\n"));
  copyFileSync(join(ROOT, "node_modules/spessasynth_lib/dist/spessasynth_processor.min.js"), join(dst, "spessasynth_processor.min.js"));
  const wasm = join(ROOT, "target/wasm32-unknown-unknown/fast/gmgen_wasm.wasm");
  if (!existsSync(wasm)) throw new Error("build gmgen-wasm first: cargo build --profile fast -p gmgen-wasm --target wasm32-unknown-unknown");
  copyFileSync(wasm, join(dst, "gmgen.wasm"));
}

if (import.meta.main) {
  mkdirSync(OUT, { recursive: true });
  const s = songs(), w = waves(), st = stats();
  writeFileSync(join(OUT, "songs.json"), JSON.stringify({ audio: BUCKET, songs: s }));
  writeFileSync(join(OUT, "waves.json"), JSON.stringify({ audio: BUCKET, waves: w }));
  writeFileSync(join(OUT, "stats.json"), JSON.stringify(st, null, 1));
  await assets();
  console.log(`site: ${s.length} songs, ${w.length} waves, gm A/B/C ${st.gm.A}/${st.gm.B}/${st.gm.C}; live.js, processor, gmgen.wasm`);
}
