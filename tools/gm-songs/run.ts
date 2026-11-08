// Full-song A/B of the generated GM bank against Windows gm.dls (change add-procedural-gm-bank).
//
//   bun tools/gm-songs/run.ts select [--gus N]   curated MIDI → build/gm/songs/selection.json
//   bun tools/gm-songs/run.ts prepare            build/gm/songs/banks/{orig,gen}.dls (named waves) + renderer self-check
//   bun tools/gm-songs/run.ts render [--only re] [--shard i/n] [--force] mix + per-channel solo renders through both banks, metrics → results.json
//   bun tools/gm-songs/run.ts waves              build/gm/songs/waves/NNN-{o,s,g,p}.mp3 from `gmfit [--set plus] dump`
//   bun tools/gm-songs/run.ts page               build/gm/songs/{index.html,waves.html,report.md}
//   bun tools/gm-songs/run.ts serve [port]       serve build/gm/songs with Range support (seeking and same-position
//                                                bank switching need it; servers that answer 200 to Range break both)
//
// Four banks (change add-gmplus-bank): o = Windows gm.dls, s = gmplus.sf2 (GMPLUS_SF2), g = gen from gm.dls
// (build/gm/gen.dls), p = gen from gmplus (build/gm-plus/gen.dls, `gmfit --set plus bank -o build/gm-plus/gen.dls`).
//
// Inputs (git-ignored, never committed): build/gm/midi/gus (archive.org BESTMID6, "The Very Best of GUS MIDI"),
// build/gm/midi/novak (archive.org DOS-Gaming-General-MIDI-Comparison midi-files.zip), build/gm/orig.dls
// (`gmfit bank --orig`, GM_DLS re-written by the dls writer), build/gm/gen.dls (`gmfit bank`), build/gm/songs/gmrender
// (`swiftc -O tools/gm-songs/render.swift`). Both banks play through the same renderer (Apple AUMIDISynth).
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { availableParallelism } from "node:os";
import { basename, dirname, join, relative } from "node:path";
import { serve } from "./serve";
import { loudness, octaveLtas, readWav, writeWav, type Audio } from "./metrics.ts";
import { BANDS, dist } from "./dist.ts";
import { nameWaves, pairsOf, rightPass, waveData } from "./dlsname.ts";
import { eventText, parse, parts, solo, trim, type Part } from "./smf.ts";

const ROOT = join(import.meta.dir, "../..");
const MIDI = join(ROOT, "build/gm/midi"), OUT = join(ROOT, "build/gm/songs");
const RATE = 44100, TAIL = 2 * RATE, EXCERPT = 30;
const arg = (n: string) => { const i = process.argv.indexOf(n); return i > 0 ? process.argv[i + 1] : undefined; };

type Sel = { id: string; src: "gus" | "novak"; file: string; title: string; seconds: number; parts: Part[] };

// ---------- bank structure + wave scores ----------
type Rgn = { key: [number, number]; wave: number };
type Ins = { drums: boolean; bank: [number, number]; program: number; name: string; regions: Rgn[] };
const structure = JSON.parse(readFileSync(join(ROOT, "assets/gm/structure.json"), "utf8")) as { instruments: Ins[] };
const scores = JSON.parse(readFileSync(join(ROOT, "assets/gm/scores.json"), "utf8")) as Record<string, { score: number }>;
/** gen-plus waves that hold a fit (not a placeholder): the gen-plus column is only meaningful where these play. */
const plusFitted = new Set(readdirSync(join(ROOT, "assets/gm-plus/waves")).filter((f) => f.endsWith(".json"))
  .filter((f) => !JSON.parse(readFileSync(join(ROOT, "assets/gm-plus/waves", f), "utf8")).placeholder).map((f) => +f.slice(0, 3)));
/** Bank key → file suffix, label. */
const BANK = { o: ["orig", "gm.dls"], s: ["sf", "gmplus"], g: ["gen", "gen-gm"], p: ["plus", "gen-plus"] } as const;
type B = keyof typeof BANK;
const KEYS = Object.keys(BANK) as B[];

function instrument(p: Part, prog: number): Ins | undefined {
  const all = structure.instruments.filter((i) => i.drums === p.drums && i.bank[0] === 0 && i.bank[1] === 0);
  return all.find((i) => i.program === prog) ?? (p.drums ? all.find((i) => i.program === 0) : undefined);
}
/** Waves a part plays (notes per wave) through the instruments of its programs. */
function wavesOf(p: Part): Record<number, number> {
  const w: Record<number, number> = {};
  const progs = Object.entries(p.programs).map(([k, n]) => [+k, n] as const);
  for (const [prog, pn] of progs) {
    const ins = instrument(p, prog);
    if (!ins) continue;
    for (const [k, kn] of Object.entries(p.keys)) {
      const r = ins.regions.find((r) => +k >= r.key[0] && +k <= r.key[1]);
      if (r) w[r.wave] = (w[r.wave] ?? 0) + (kn * pn) / p.notes;
    }
  }
  return w;
}
const mainProg = (p: Part) => +Object.entries(p.programs).sort((a, b) => b[1] - a[1])[0][0];
function label(p: Part): string {
  const ins = instrument(p, mainProg(p));
  return `${p.drums ? "Drums" : `ch${p.ch + 1}`} ${p.drums ? "kit" : "prog"} ${mainProg(p)} ${ins?.name.trim() ?? "?"}`;
}

// ---------- select ----------
function walk(d: string): string[] {
  return readdirSync(d).flatMap((f) => (statSync(join(d, f)).isDirectory() ? walk(join(d, f)) : /\.mid$/i.test(f) ? [join(d, f)] : []));
}
function descriptions(): Map<string, string> {
  const m = new Map<string, string>();
  for (const f of walk(join(MIDI, "gus")).map(dirname).filter((d, i, a) => a.indexOf(d) === i)) {
    const p = join(f, "DESCRIPT.ION");
    if (!existsSync(p)) continue;
    for (const l of readFileSync(p, "latin1").split(/\r?\n/)) {
      const [name, ...rest] = l.split(" ");
      if (name && rest.length && rest.join(" ").trim() !== "?") m.set(name.toLowerCase(), rest.join(" ").trim());
    }
  }
  return m;
}

function select() {
  const nGus = +(arg("--gus") ?? 24), desc = descriptions(), seen = new Set<string>();
  const load = (file: string, src: Sel["src"]): Sel | undefined => {
    const bytes = readFileSync(file), h = createHash("md5").update(bytes).digest("hex");
    if (seen.has(h)) return;
    seen.add(h);
    try {
      const s = trim(parse(new Uint8Array(bytes), RATE));
      const name = basename(file).replace(/\.mid$/i, "");
      return { id: `${src}-${name.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`, src, file: relative(ROOT, file), seconds: Math.round(s.seconds),
        title: src === "novak" ? name.replace(/-/g, " ") : desc.get(basename(file).toLowerCase()) ?? name, parts: parts(s.events) };
    } catch { return; }
  };
  const novak = walk(join(MIDI, "novak")).filter((f) => !/test-tone/.test(f)).sort().map((f) => load(f, "novak")).filter((s) => s) as Sel[];
  // GUS: greedy coverage. A new melodic program counts 1, a new drum key 0.25; 45 s – 8 min, ≥ 3 channels.
  const pool = walk(join(MIDI, "gus")).sort().map((f) => load(f, "gus"))
    .filter((s): s is Sel => !!s && s.seconds >= 45 && s.seconds <= 480 && s.parts.length >= 3);
  const progs = new Set<number>(), keys = new Set<number>(), gus: Sel[] = [];
  const gainOf = (s: Sel) => s.parts.reduce((g, p) => g + (p.drums
    ? Object.keys(p.keys).filter((k) => !keys.has(+k)).length * 0.25
    : Object.keys(p.programs).filter((k) => !progs.has(+k)).length), 0) + s.parts.length * 0.01;
  while (gus.length < nGus && pool.length) {
    pool.sort((a, b) => gainOf(b) - gainOf(a));
    const s = pool.shift()!;
    gus.push(s);
    for (const p of s.parts) for (const k of Object.keys(p.drums ? p.keys : p.programs)) (p.drums ? keys : progs).add(+k);
  }
  const sel = [...novak, ...gus];
  mkdirSync(OUT, { recursive: true });
  writeFileSync(join(OUT, "selection.json"), JSON.stringify(sel, null, 1));
  const allProgs = new Set(sel.flatMap((s) => s.parts.filter((p) => !p.drums).flatMap((p) => Object.keys(p.programs).map(Number))));
  const allKeys = new Set(sel.flatMap((s) => s.parts.filter((p) => p.drums).flatMap((p) => Object.keys(p.keys).map(Number))));
  console.log(`${novak.length} Novak + ${gus.length} GUS songs; ${allProgs.size}/128 programs, ${allKeys.size} drum keys`);
}

// ---------- render ----------
async function pool<T>(items: T[], jobs: number, f: (t: T) => Promise<void>) {
  let i = 0;
  await Promise.all(Array.from({ length: jobs }, async () => { while (i < items.length) await f(items[i++]); }));
}
async function render(bank: string, events: string, wav: string) {
  const p = Bun.spawn([join(OUT, "gmrender"), bank.startsWith("/") ? bank : join(ROOT, "build/gm", bank), events, wav], { stderr: "pipe" });
  if ((await p.exited) !== 0) throw new Error(`gmrender ${bank} ${events}: ${await new Response(p.stderr).text()}`);
}
async function mp3(a: Audio, gain: number, path: string, from = 0, secs = Infinity) {
  const s = Math.round(from * a.rate), e = Math.min(a.ch[0].length, s + Math.round(secs * a.rate));
  const tmp = path.replace(/\.mp3$/, ".tmp.wav");
  writeWav(tmp, { rate: a.rate, ch: a.ch.map((c) => c.slice(s, e).map((x) => x * gain)) });
  const p = Bun.spawn(["lame", "--quiet", "-V5", tmp, path]);
  await p.exited;
  rmSync(tmp);
}
const peak = (a: Audio) => a.ch.reduce((m, c) => c.reduce((m, x) => Math.max(m, Math.abs(x)), m), 0);
function metrics(gen: Audio, orig: Audio) {
  const d = dist(gen, orig), lo = loudness(orig), lg = loudness(gen);
  const oo = octaveLtas(orig), og = octaveLtas(gen);
  return { lufs_orig: +lo.toFixed(2), lufs_gen: +lg.toFixed(2), d_lufs: +(lg - lo).toFixed(2), tf: +d.tf.toFixed(2),
    gain: +d.gain.toFixed(2), octaves: oo.map((b, i) => ({ fc: b.fc, d: +(og[i].db - b.db).toFixed(1) })),
    bands: d.bands.map((x) => +x.toFixed(1)) };
}

/** Named copies of both banks for AUMIDISynth (see dlsname.ts), plus a mapping self-check: an Organ 1 note must
 * go silent when its own wave (150) is zeroed, i.e. the renderer plays the wave the region links to. */
const PROBE_WAVE = 150, PROBE = "len 88200\n0 192 16\n4410 144 60 100\n60000 128 60 0\n"; // Organ 1, C4
const BANKS = join(OUT, "banks");
const bankFile = (b: B) => join(BANKS, b === "s" ? "gmplus.sf2" : `${BANK[b][0]}.dls`);
async function prepareBanks() {
  mkdirSync(BANKS, { recursive: true });
  const src = { orig: "build/gm/orig.dls", gen: "build/gm/gen.dls", plus: "build/gm-plus/gen.dls" };
  for (const [b, f] of Object.entries(src)) writeFileSync(join(BANKS, `${b}.dls`), nameWaves(readFileSync(join(ROOT, f))));
  const plus = readFileSync(join(BANKS, "plus.dls"));
  writeFileSync(join(BANKS, "plus-R.dls"), rightPass(plus));
  console.log(`gen-plus: ${pairsOf(plus).length} stereo pairs (two-pass render)`);
  const sf = process.env.GMPLUS_SF2;
  if (!sf || !existsSync(sf)) throw new Error("GMPLUS_SF2 must name gmplus.sf2 (never committed)");
  writeFileSync(bankFile("s"), readFileSync(sf));
  const tmp = join(OUT, `probe-${process.pid}`);
  mkdirSync(tmp, { recursive: true });
  writeFileSync(join(tmp, "probe.txt"), PROBE);
  // every DLS copy: an Organ 1 note sounds, and goes silent with its wave zeroed
  for (const b of ["orig", "gen", "plus"]) {
    const z = readFileSync(join(BANKS, `${b}.dls`)), [at, n] = waveData(z)[PROBE_WAVE];
    z.fill(0, at, at + n);
    writeFileSync(join(tmp, "z.dls"), z);
    await render(join(BANKS, `${b}.dls`), join(tmp, "probe.txt"), join(tmp, "a.wav"));
    await render(join(tmp, "z.dls"), join(tmp, "probe.txt"), join(tmp, "z.wav"));
    const [a, zz] = [readWav(join(tmp, "a.wav")), readWav(join(tmp, "z.wav"))];
    if (!(peak(a) > 0.01 && peak(zz) < 1e-4)) throw new Error(`${b}: renderer does not play the linked wave (peaks ${peak(a)}, zeroed ${peak(zz)})`);
  }
  await render(bankFile("s"), join(tmp, "probe.txt"), join(tmp, "a.wav"));
  if (!(peak(readWav(join(tmp, "a.wav"))) > 0.01)) throw new Error("gmplus.sf2: the probe note is silent");
  // two-pass stereo is exact only if a render is deterministic: the same bank twice must be bit-identical
  await render(join(BANKS, "orig.dls"), join(tmp, "probe.txt"), join(tmp, "b1.wav"));
  await render(join(BANKS, "orig.dls"), join(tmp, "probe.txt"), join(tmp, "b2.wav"));
  if (!readFileSync(join(tmp, "b1.wav")).equals(readFileSync(join(tmp, "b2.wav")))) throw new Error("renderer is not deterministic: two-pass stereo would be wrong");
  rmSync(tmp, { recursive: true });
}

async function renderAll() {
  if (!KEYS.every((b) => existsSync(bankFile(b)))) throw new Error("run `run.ts prepare` first");
  const sel = JSON.parse(readFileSync(join(OUT, "selection.json"), "utf8")) as Sel[];
  const only = arg("--only") ? new RegExp(arg("--only")!) : undefined;
  // --shard i/n: this process takes every n-th song from i and writes results.<i>.json (page merges results*.json)
  const [si, sn] = (arg("--shard") ?? "0/1").split("/").map(Number);
  const resPath = join(OUT, sn > 1 ? `results.${si}.json` : "results.json");
  const results: Record<string, unknown> = existsSync(resPath) ? JSON.parse(readFileSync(resPath, "utf8")) : {};
  // songs that already hold four-way results are skipped unless --force (a crashed run resumes)
  const done4 = (id: string) => !!(results[id] as { mix?: { plus?: unknown } } | undefined)?.mix?.plus;
  const todo = sel.filter((s, i) => (!only || only.test(s.id)) && i % sn === si && (process.argv.includes("--force") || !done4(s.id)));
  const jobs = +(process.env.JOBS ?? Math.max(1, availableParallelism() - 1));
  let done = 0;
  await pool(todo, jobs, async (s) => {
    const dir = join(OUT, s.id), tmp = join(dir, "tmp");
    mkdirSync(tmp, { recursive: true });
    const song = trim(parse(new Uint8Array(readFileSync(join(ROOT, s.file))), RATE));
    const len = song.frames + TAIL;
    // all four banks; one shared gain per row so loudness differences stay audible
    const quad = async (name: string, events: typeof song.events) => {
      writeFileSync(join(tmp, `${name}.txt`), eventText(events, len));
      const out = {} as Record<B, Audio>;
      const pass = async (bank: string, tag: string) => {
        const f = join(tmp, `${name}-${tag}.wav`);
        await render(bank, join(tmp, `${name}.txt`), f);
        const a = readWav(f);
        rmSync(f); // float WAVs of 16 stems × 4 banks fill gigabytes per song
        return a;
      };
      for (const b of KEYS) out[b] = await pass(bankFile(b), b);
      // gen-plus stereo pairs: left output of the bank, right output of its right-channel copy
      if (existsSync(join(BANKS, "plus-R.dls"))) out.p = { rate: out.p.rate, ch: [out.p.ch[0], (await pass(join(BANKS, "plus-R.dls"), "pR")).ch[1]] };
      return out;
    };
    const encode = async (a: Record<B, Audio>, name: string, from = 0, secs = Infinity) => {
      const g = 0.89 / Math.max(...KEYS.map((b) => peak(a[b])), 1e-9);
      for (const b of KEYS) await mp3(a[b], g, join(dir, `${name}-${BANK[b][0]}.mp3`), from, secs);
    };
    /** gen↔gm.dls (top-level fields, as before), gen-plus↔gmplus, gmplus↔gm.dls */
    const four = (a: Record<B, Audio>) => ({ ...metrics(a.g, a.o), plus: metrics(a.p, a.s), sf: metrics(a.s, a.o) });
    const m4 = await quad("mix", song.events);
    await encode(m4, "mix");
    const mix = four(m4);
    const stems = [];
    for (const p of s.parts) {
      const s4 = await quad(`ch${p.ch}`, solo(song.events, p.ch));
      const first = song.events.find((e) => (e.bytes[0] & 0xf0) === 0x90 && e.bytes[2] && (e.bytes[0] & 15) === p.ch)!.frame / RATE;
      await encode(s4, `ch${p.ch}`, Math.max(0, first - 0.3), EXCERPT);
      const waves = wavesOf(p);
      const tot = Object.values(waves).reduce((a, b) => a + b, 0) || 1;
      const bankScore = Object.entries(waves).reduce((a, [w, n]) => a + (scores[w]?.score ?? 0) * n, 0) / tot;
      stems.push({ ch: p.ch, label: label(p), drums: p.drums, program: mainProg(p), notes: p.notes,
        waves: Object.fromEntries(Object.entries(waves).map(([w, n]) => [w, +n.toFixed(1)])), bank_score: +bankScore.toFixed(3),
        ...four(s4) });
    }
    rmSync(tmp, { recursive: true });
    results[s.id] = { ...s, mix, stems };
    writeFileSync(resPath, JSON.stringify(results));
    console.log(`[${++done}/${todo.length}] ${s.id}: ΔLUFS ${mix.d_lufs} tf ${mix.tf}`);
  });
}

// ---------- page + report ----------
type St = { ch: number; label: string; drums: boolean; program: number; notes: number; waves: Record<string, number>; bank_score: number;
  d_lufs: number; tf: number; lufs_orig: number; octaves: { fc: number; d: number }[]; bands: number[]; plus?: St; sf?: St };
type Res = Sel & { mix: St; stems: St[] };
const esc = (s: string) => s.replace(/[&<>"]/g, (c) => `&#${c.charCodeAt(0)};`);
const mean = (a: number[]) => a.reduce((x, y) => x + y, 0) / (a.length || 1);
function corr(x: number[], y: number[]) {
  const mx = mean(x), my = mean(y);
  let sxy = 0, sxx = 0, syy = 0;
  x.forEach((v, i) => { sxy += (v - mx) * (y[i] - my); sxx += (v - mx) ** 2; syy += (y[i] - my) ** 2; });
  return sxy / Math.sqrt(sxx * syy);
}

function page() {
  const files = readdirSync(OUT).filter((f) => /^results(\.\d+)?\.json$/.test(f));
  const all: Record<string, Res> = Object.assign({}, ...files.map((f) => JSON.parse(readFileSync(join(OUT, f), "utf8"))));
  const res = Object.values(all)
    .sort((a, b) => a.src.localeCompare(b.src) || a.id.localeCompare(b.id));
  // stems that matter in the mix: within 20 dB of the song mix loudness
  const stems = res.flatMap((r) => r.stems.filter((s) => Number.isFinite(s.tf) && s.lufs_orig > r.mix.lufs_orig - 20).map((s) => ({ r, s })));
  const byIns = new Map<string, { r: Res; s: St }[]>();
  for (const x of stems) {
    const k = x.s.drums ? `Drums (kit ${x.s.program})` : `${x.s.program} ${x.s.label.split(/prog \d+ /)[1]}`;
    byIns.set(k, [...(byIns.get(k) ?? []), x]);
  }
  const ins = [...byIns.entries()].map(([k, xs]) => ({ k, n: xs.length, tf: mean(xs.map((x) => x.s.tf)), dl: mean(xs.map((x) => x.s.d_lufs)),
    bank: mean(xs.map((x) => x.s.bank_score)), waves: [...new Set(xs.flatMap((x) => Object.keys(x.s.waves)))].slice(0, 8) }))
    .sort((a, b) => b.tf - a.tf);
  const fmt = (x: number, d = 2) => (Number.isFinite(x) ? x.toFixed(d) : "–");
  const rMix = corr(stems.map((x) => x.s.bank_score), stems.map((x) => x.s.tf));
  const md = [
    "# GM bank: full-song A/B (generated vs Windows gm.dls)", "",
    `Renderer: Apple AUMIDISynth (same for both banks), 44.1 kHz float, no reverb. Songs: ${res.length} ` +
      `(${res.filter((r) => r.src === "novak").length} Novak DOS-game set, ${res.filter((r) => r.src === "gus").length} Very Best of GUS MIDI).`,
    "`tf` = mean |Δ dB| of third-octave band levels per 46 ms frame after removing the overall gain (timbre over time; 0 = identical). " +
      "`ΔLUFS` = generated − original loudness.", "",
    `Mixes: median tf ${fmt(median(res.map((r) => r.mix.tf)))}, median ΔLUFS ${fmt(median(res.map((r) => r.mix.d_lufs)))}; ` +
      `stems (within 20 dB of the mix): ${stems.length}, corr(bank score, tf) = ${fmt(rMix)}.`, "",
    "## Instruments ranked (worst first)", "", "| instrument | parts | tf | ΔLUFS | gmfit score | waves |", "|---|---|---|---|---|---|",
    ...ins.map((i) => `| ${i.k} | ${i.n} | ${fmt(i.tf)} | ${fmt(i.dl)} | ${fmt(i.bank, 3)} | ${i.waves.join(" ")} |`), "",
    "## Songs", "", "| song | s | ΔLUFS | tf | worst stems |", "|---|---|---|---|---|",
    ...res.map((r) => `| ${r.id} | ${r.seconds} | ${fmt(r.mix.d_lufs)} | ${fmt(r.mix.tf)} | ${[...r.stems].sort((a, b) => b.tf - a.tf).slice(0, 3)
      .map((s) => `${s.label} (${fmt(s.tf, 1)})`).join("; ")} |`), "",
  ].join("\n");
  writeFileSync(join(OUT, "report.md"), md);

  const cov = (st: St) => { // share of the stem's notes on gen-plus waves that hold a fit
    const t = Object.values(st.waves).reduce((x, y) => x + y, 0);
    return t ? Object.entries(st.waves).reduce((x, [w, n]) => x + (plusFitted.has(+w) ? n : 0), 0) / t : 0;
  };
  const tfc = (x?: number) => `<td class="${x === undefined ? "" : x > 6 ? "bad" : x > 4 ? "warn" : ""}">${fmt(x ?? NaN)}</td>`;
  const ab = (id: string, n: string, c = 1) => `<span class="ab" ${KEYS.map((b) => `data-${b}="${id}/${n}-${BANK[b][0]}.mp3"`).join(" ")}>` +
    KEYS.map((b, k) => `<button data-w="${b}" title="key ${k + 1}"${b === "p" && c < 0.5 ? ' class="dim"' : ""}>${BANK[b][1]}</button>`).join("") + "</span>";
  const row = (st: St & { plus?: St; sf?: St }, name: string, id: string, c: number) =>
    `${tfc(st.tf)}${tfc(st.plus?.tf)}${tfc(st.sf?.tf)}<td>${fmt(st.d_lufs)}</td><td>${fmt(st.plus?.d_lufs ?? NaN)}</td><td>${(c * 100).toFixed(0)}%</td>`;
  const head = `<th>tf gen-gm↔gm.dls</th><th>tf gen-plus↔gmplus</th><th>tf gmplus↔gm.dls</th><th>ΔLUFS gen-gm</th><th>ΔLUFS gen-plus</th><th title="notes on fitted gen-plus waves">plus fitted</th>`;
  const html = `<!doctype html><meta charset="utf-8"><title>GM banks: four-way song A/B</title>
${STYLE}
<div id="bar"><b>GM banks — songs</b> · <a href="waves.html">sounds (495 waves) →</a><br>${PLAYER_BAR}</div>
<p>gm.dls = Windows bank · gmplus = SC-55-based soundfont · gen-gm = procedural, fitted to gm.dls · gen-plus = procedural, fitted to gmplus
(<b>${[...plusFitted].filter((w) => w < 495).length}/495 waves fitted</b>${[...plusFitted].some((w) => w >= 495) ? `, ${[...plusFitted].filter((w) => w >= 495).length} stereo partners` : ""}; until all are fitted, trust the gen-plus column only where "plus fitted" is high).
Same renderer (AUMIDISynth) for all four; one gain per row. <code>tf</code>: timbre-over-time distance, 0 = identical.</p>
${SOURCES}
<pre>${esc(md.split("## Instruments")[0])}</pre>
<h2>Instruments ranked by gen-gm tf (worst first)</h2><table><tr><th>instrument</th><th>parts</th><th>tf gen-gm</th><th>ΔLUFS</th><th>gmfit</th><th class="l">waves</th></tr>
${ins.map((i) => `<tr><td>${esc(i.k)}</td><td>${i.n}</td>${tfc(i.tf)}<td class="${Math.abs(i.dl) > 3 ? "bad" : ""}">${fmt(i.dl)}</td><td>${fmt(i.bank, 3)}</td><td class="l">${i.waves.join(" ")}</td></tr>`).join("\n")}</table>
<h2>Songs</h2>
${res.map((r) => { const mc = cov({ ...r.mix, waves: Object.assign({}, ...r.stems.map((s) => s.waves)) } as St);
  return `<details><summary><b>${esc(r.title)}</b> <small>${r.src} · ${r.seconds}s · tf gen-gm ${fmt(r.mix.tf)} · gen-plus ${fmt((r.mix as any).plus?.tf ?? NaN)}</small> ${ab(r.id, "mix", mc)}</summary>
<table><tr><th>stem (30 s excerpt)</th><th></th><th>notes</th>${head}<th class="l">waves</th></tr>
<tr><td><b>mix</b></td><td>${ab(r.id, "mix", mc)}</td><td></td>${row(r.mix, "mix", r.id, mc)}<td></td></tr>
${[...r.stems].sort((a, b) => b.tf - a.tf).map((st) => { const c = cov(st);
  return `<tr><td>${esc(st.label)}</td><td>${ab(r.id, `ch${st.ch}`, c)}</td><td>${st.notes}</td>${row(st, `ch${st.ch}`, r.id, c)}<td class="l">${Object.keys(st.waves).map((w) => `<a href="waves.html#w${w}">${w}</a>`).join(" ")}</td></tr>`; }).join("\n")}
</table></details>`; }).join("\n")}
${PLAYER_JS}`;
  writeFileSync(join(OUT, "index.html"), html);
  console.log(`${join(OUT, "index.html")}\n${join(OUT, "report.md")}`);
  if (existsSync(join(WAVES, "gm/index.json"))) wavesPage();
}
// Where the rendered material comes from (the renders are ours; the inputs are public downloads, never re-hosted).
export const SOURCES = `<p><small>Sources: MIDI — John Novak, <i>Grand MS-DOS gaming General MIDI showdown</i>
(<a href="https://archive.org/details/DOS-Gaming-General-MIDI-Comparison">archive.org</a>) and <i>The Very Best of GUS MIDI</i>
(<a href="https://archive.org/details/BESTMID6">archive.org BESTMID6</a>). Banks — Windows <code>gm.dls</code> (© Microsoft / Roland,
read from a local copy) and gmplus (fan soundfont from Roland Sound Canvas samples, Release 1, by Distortion Dog).
All audio rendered by <a href="https://github.com/unisynth/gm-procedural">gm-procedural</a> for comparison only.</small></p>`;
const STYLE = `<style>.ab{white-space:nowrap}body{font:13px system-ui;margin:20px;background:#111;color:#ddd}a{color:#8cf}table{border-collapse:collapse;margin:6px 0 18px}
td,th{padding:2px 6px;border-bottom:1px solid #333;text-align:right}td:first-child,th:first-child,td.l,th.l{text-align:left}.bad{color:#f66}.warn{color:#fc6}.good{color:#6d6}
button{background:#333;color:#ddd;border:1px solid #555;margin:0 1px;cursor:pointer;font-size:12px}button.on{background:#2a6;color:#fff}button.dim{opacity:.45}
h2{margin-top:28px}details{margin:4px 0}summary{cursor:pointer}#bar{position:sticky;top:0;background:#111;padding:6px 0;border-bottom:1px solid #333;z-index:1}
#seek{width:420px;vertical-align:middle}tr:target{background:#233}</style>`;
const PLAYER_BAR = `<button id="pp">⏯</button> <input id="seek" type="range" min="0" max="1" step="0.001" value="0"> <span id="t">0:00</span>
· keys <b>1–4</b> switch bank at the same position, <b>space</b> play/pause, <b>←/→</b> ±5 s, <b>L</b> loop <span id="now"></span>`;
/** One player for the page: switching bank keeps the position, so the four versions stay in sync. */
const PLAYER_JS = `<script>
const au = new Audio(); let cur = null; const K = ${JSON.stringify(KEYS)};
const seek = document.getElementById("seek"), tt = document.getElementById("t");
function play(span, w) {
  const t = cur === span ? au.currentTime : 0, wasLoop = au.loop;
  document.querySelectorAll("button.on").forEach(b => b.classList.remove("on"));
  cur = span; au.src = span.dataset[w]; au.loop = wasLoop;
  au.addEventListener("loadedmetadata", () => { au.currentTime = Math.min(t, au.duration - 0.01); }, { once: true });
  au.play(); span.querySelectorAll('[data-w="' + w + '"]').forEach(b => b.classList.add("on"));
  document.getElementById("now").textContent = " ▶ " + span.dataset[w];
}
au.ontimeupdate = () => { if (au.duration) seek.value = au.currentTime / au.duration; tt.textContent = Math.floor(au.currentTime / 60) + ":" + String(Math.floor(au.currentTime % 60)).padStart(2, "0"); };
seek.oninput = () => { if (au.duration) au.currentTime = seek.value * au.duration; };
document.getElementById("pp").onclick = () => au.paused ? au.play() : au.pause();
document.addEventListener("click", e => { const b = e.target.closest("button[data-w]"); if (b) { e.preventDefault(); play(b.closest(".ab"), b.dataset.w); } });
document.addEventListener("keydown", e => {
  if (e.target.tagName === "INPUT") return;
  if (e.key === " ") { au.paused ? au.play() : au.pause(); e.preventDefault(); }
  if (e.key === "ArrowLeft") au.currentTime = Math.max(0, au.currentTime - 5);
  if (e.key === "ArrowRight") au.currentTime += 5;
  if (e.key === "l" || e.key === "L") au.loop = !au.loop;
  const k = +e.key - 1; if (cur && K[k]) play(cur, K[k]);
});
</script>`;

// ---------- sounds: every wave, four versions ----------
type WRow = { wave: number; category: string; instruments: string[]; placeholder: boolean; score: number; tier: string; flags: string[]; source?: string };
const WAVES = join(OUT, "waves");
/** gmfit dump output (gm/ and plus/) → NNN-{orig,sf,gen,plus}.mp3. o/g from the gm set, s/p from the plus set. */
async function waves() {
  const jobs: [string, string][] = [];
  for (let i = 0; i < 495; i++) {
    const n = String(i).padStart(3, "0");
    jobs.push([join(WAVES, "gm", `${n}-orig.wav`), join(WAVES, `${n}-orig.mp3`)], [join(WAVES, "gm", `${n}-gen.wav`), join(WAVES, `${n}-gen.mp3`)],
      [join(WAVES, "plus", `${n}-orig.wav`), join(WAVES, `${n}-sf.mp3`)], [join(WAVES, "plus", `${n}-gen.wav`), join(WAVES, `${n}-plus.mp3`)]);
  }
  if (!existsSync(jobs[0][0]) || !existsSync(jobs[2][0])) throw new Error("run `gmfit dump build/gm/songs/waves/gm` and `gmfit --set plus dump build/gm/songs/waves/plus` first");
  // stereo waves (change add-gmplus-bank D4): gmplus target and gen-plus as left wave + partner, one stereo file
  const map = JSON.parse(readFileSync(join(ROOT, "assets/gm-plus/map.json"), "utf8")).waves as { wave: number; stereo?: { partner: number } }[];
  const partner = new Map(map.filter((r) => r.stereo).map((r) => [r.wave, r.stereo!.partner]));
  const pad = (i: number) => String(i).padStart(3, "0");
  for (const [l, r] of partner) {
    for (const kind of ["orig", "gen"]) {
      const [a, b] = [readWav(join(WAVES, "plus", `${pad(l)}-${kind}.wav`)), readWav(join(WAVES, "plus", `${pad(r)}-${kind}.wav`))];
      const st = join(WAVES, "plus", `${pad(l)}-${kind}.st.wav`);
      writeWav(st, { rate: a.rate, ch: [a.ch[0], b.ch[0]] });
      const j = jobs.find((x) => x[0] === join(WAVES, "plus", `${pad(l)}-${kind}.wav`))!;
      j[0] = st;
    }
  }
  await pool(jobs, availableParallelism(), async ([a, b]) => { await Bun.spawn(["lame", "--quiet", "-V4", a, b]).exited; });
  console.log(`${jobs.length} mp3 → ${WAVES}`);
}
function wavesPage() {
  const gm = JSON.parse(readFileSync(join(WAVES, "gm/index.json"), "utf8")) as WRow[];
  const plus = JSON.parse(readFileSync(join(WAVES, "plus/index.json"), "utf8")) as WRow[];
  const map = JSON.parse(readFileSync(join(ROOT, "assets/gm-plus/map.json"), "utf8")).waves as { wave: number; stereo?: { partner: number } }[];
  const stereo = new Set(map.filter((r) => r.stereo).map((r) => r.wave)), partnerCount = stereo.size;
  const fmt = (x: number) => (Number.isFinite(x) ? x.toFixed(3) : "–");
  const cats = [...new Set(gm.map((r) => r.category))];
  const ab = (i: number, ph: boolean) => `<span class="ab" ${KEYS.map((b) => `data-${b}="waves/${String(i).padStart(3, "0")}-${BANK[b][0]}.mp3"`).join(" ")}>` +
    KEYS.map((b, k) => `<button data-w="${b}" title="key ${k + 1}"${b === "p" && ph ? ' class="dim"' : ""}>${BANK[b][1]}</button>`).join("") + "</span>";
  const html = `<!doctype html><meta charset="utf-8"><title>GM banks: four-way sound A/B</title>
${STYLE}
<div id="bar"><b>GM banks — sounds</b> · <a href="index.html">← songs</a> · filter <select id="cat"><option value="">all categories</option>${cats.map((c) => `<option>${esc(c)}</option>`).join("")}</select>
<label><input type="checkbox" id="fitted"> only gen-plus fitted (${plusFitted.size})</label><br>${PLAYER_BAR}</div>
<p>Each wave played as the bank stores it (loop repeated to 2 s), at its own rate: gm.dls and gen-gm at 22.05 kHz, gmplus target and gen-plus at 32 kHz.
The ${partnerCount} waves marked <b>stereo</b> play gmplus's panned layers and gen-plus's partner wave on the right channel.
gmplus = the target gmfit derives from gmplus.sf2 for this wave (filter + mod envelope baked, level-matched). Score: gmfit, 1 = identical to its own target.</p>
${SOURCES}
<table id="tw"><tr><th>wave</th><th class="l">instruments</th><th class="l">category</th><th></th><th>gen-gm score</th><th>gen-plus score</th><th class="l">gmplus sample</th><th class="l">flags gen-gm / gen-plus</th></tr>
${gm.map((g, i) => { const p = plus[i];
  return `<tr id="w${i}" data-cat="${esc(g.category)}" data-fit="${p.placeholder ? 0 : 1}"><td><a href="#w${i}">${i}</a></td><td class="l">${esc(g.instruments.slice(0, 3).join(", "))}${g.instruments.length > 3 ? ` +${g.instruments.length - 3}` : ""}</td>
<td class="l">${esc(g.category)}${stereo.has(i) ? " <b>stereo</b>" : ""}</td><td>${ab(i, p.placeholder)}</td><td>${g.placeholder ? "–" : fmt(g.score)}</td><td class="${p.placeholder ? "" : p.score > g.score ? "good" : "bad"}">${p.placeholder ? "placeholder" : fmt(p.score)}</td>
<td class="l">${esc(String(p.source ?? "–"))}</td><td class="l">${esc((g.flags ?? []).join(" "))} / ${esc((p.flags ?? []).join(" "))}</td></tr>`; }).join("\n")}
</table>
<script>
const cat = document.getElementById("cat"), fit = document.getElementById("fitted");
function filt() { document.querySelectorAll("#tw tr[data-cat]").forEach(r => r.style.display = (!cat.value || r.dataset.cat === cat.value) && (!fit.checked || r.dataset.fit === "1") ? "" : "none"); }
cat.onchange = fit.onchange = filt;
</script>
${PLAYER_JS}`;
  writeFileSync(join(OUT, "waves.html"), html);
  console.log(join(OUT, "waves.html"));
}
function median(a: number[]) { const s = a.filter(Number.isFinite).sort((x, y) => x - y); return s[Math.floor(s.length / 2)] ?? NaN; }

const cmd = process.argv[2];
if (cmd === "select") select();
else if (cmd === "prepare") await prepareBanks();
else if (cmd === "render") await renderAll();
else if (cmd === "waves") await waves();
else if (cmd === "page") page();
else if (cmd === "serve") console.log(`http://localhost:${serve(OUT, Number(process.argv[3] ?? 60941)).port}/`);
else { console.error("usage: run.ts select|prepare|render|waves|page"); process.exit(1); }
