// Site audio + metrics with the same engine the site plays live (spessasynth_core):
//   bun tools/spessa/render.ts songs [--only re] [--part i/n] [--max n] [--force]   → build/site-audio/songs/<id>/{mix,ch<N>}.mp3 (gm.dls only) + build/site-audio/results.json
//   bun tools/spessa/render.ts waves               → build/site-audio/waves/NNN.mp3 (gm.dls note per wave) + waves.json (note per wave)
// gm.dls is copyrighted, so only its renders are published; gen-gm, gmplus and gen-plus play live in the browser from
// the MIDI. Metrics (tf, ΔLUFS) still compare all four banks, rendered here and discarded.
// Banks: GM_DLS, GMPLUS_SF2, build/gm/gen.dls, build/gm-plus/gen.dls (`gmfit bank`, `gmfit --set plus bank`).
// One process, songs one after another (each holds four banks of float audio: ~0.5 GB at most).
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { BasicMIDI, SoundBankLoader, SpessaSynthProcessor, SpessaSynthSequencer, type BasicSoundBank } from "spessasynth_core";
import { dist } from "../gm-songs/dist.ts";
import { loudness, writeWav, type Audio } from "../gm-songs/metrics.ts";

const ROOT = join(import.meta.dir, "../..");
const OUT = join(ROOT, "build/site-audio");
export const RATE = 44100;
const TAIL = 2, EXCERPT = 30, Q = 128;
const ab = (p: string) => { const b = readFileSync(p); return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength) as ArrayBuffer; };
const env = (k: string) => { const v = process.env[k]; if (!v) throw new Error(`set ${k}`); return v; };

export type Key = "o" | "s" | "g" | "p";
function banks(): Record<Key, BasicSoundBank> {
  const load = (p: string) => SoundBankLoader.fromArrayBuffer(ab(p));
  return { o: load(env("GM_DLS")), s: load(env("GMPLUS_SF2")), g: load(join(ROOT, "build/gm/gen.dls")), p: load(join(ROOT, "build/gm-plus/gen.dls")) };
}

/** Renders `midi` through `bank`: optional solo channel (others muted), window [from, from + secs). */
export function renderMidi(bank: BasicSoundBank, midi: BasicMIDI, { solo, from = 0, secs = Infinity }: { solo?: number; from?: number; secs?: number } = {}): Audio {
  const synth = new SpessaSynthProcessor(RATE, { effectsEnabled: true });
  synth.soundBankManager.addSoundBank(bank, "main");
  const seq = new SpessaSynthSequencer(synth);
  seq.skipToFirstNoteOn = false;
  seq.loadNewSongList([midi]);
  seq.loopCount = 0;
  seq.play();
  if (from > 0) seq.currentTime = from;
  if (solo !== undefined) synth.midiChannels.forEach((c, i) => c.setSystemParameter("isMuted", i !== solo));
  const end = Math.min(midi.duration + TAIL, from + secs);
  const n = Math.max(0, Math.ceil((end - from) * RATE));
  const L = new Float32Array(n), R = new Float32Array(n);
  for (let i = 0; i < n; i += Q) { seq.processTick(); synth.process(L, R, i, Math.min(Q, n - i)); }
  synth.destroySynthProcessor?.();
  return { rate: RATE, ch: [L, R] };
}

async function mp3(a: Audio, path: string) {
  const tmp = path.replace(/\.mp3$/, ".tmp.wav");
  writeWav(tmp, a);
  const p = Bun.spawn(["lame", "--quiet", "-V5", tmp, path]);
  if ((await p.exited) !== 0) throw new Error(`lame failed: ${path}`);
  rmSync(tmp);
}

const r2 = (x: number) => +x.toFixed(2);
const four = (a: Record<Key, Audio>) => {
  const m = (gen: Audio, orig: Audio) => ({ tf: r2(dist(gen, orig).tf), d_lufs: r2(loudness(gen) - loudness(orig)) });
  return { ...m(a.g, a.o), plus: m(a.p, a.s), sf: m(a.s, a.o) };
};

type Sel = { id: string; file: string; parts: { ch: number; drums: boolean; notes: number }[] };

async function songs(only?: RegExp, part = "0/1", max = Infinity) {
  let rendered = 0;
  const [pi, pn] = part.split("/").map(Number);
  const B = banks();
  const sel = (JSON.parse(readFileSync(join(ROOT, "build/gm/songs/selection.json"), "utf8")) as Sel[]).filter((s, i) => (!only || only.test(s.id)) && i % pn === pi);
  // --part i/n: every n-th song, own results file (the site build merges results*.json); at most 2 parts (RAM)
  const resPath = join(OUT, pn > 1 ? `results.${pi}.json` : "results.json");
  const results: Record<string, unknown> = existsSync(resPath) ? JSON.parse(readFileSync(resPath, "utf8")) : {};
  mkdirSync(join(OUT, "midi"), { recursive: true });
  for (const [k, s] of sel.entries()) {
    if (results[s.id] && !process.argv.includes("--force")) continue; // resumable: finished songs are skipped
    // the engine keeps ~100–200 MB per song alive; --max N ends the process (exit 10 = more to do) so a loop can restart it
    if (rendered++ >= max) process.exit(10);
    const t0 = performance.now();
    const dir = join(OUT, "songs", s.id);
    mkdirSync(dir, { recursive: true });
    const bytes = readFileSync(join(ROOT, s.file));
    const midPath = join(OUT, "midi", `${s.id}.mid`);
    rmSync(midPath, { force: true }); // archive files are read-only; replace, never write through
    writeFileSync(midPath, bytes);
    const midi = BasicMIDI.fromArrayBuffer(ab(join(ROOT, s.file)));
    const keys: Key[] = ["o", "s", "g", "p"];
    const mix = Object.fromEntries(keys.map((b) => [b, renderMidi(B[b], midi)])) as Record<Key, Audio>;
    await mp3(mix.o, join(dir, "mix.mp3"));
    const mixM = four(mix);
    const stems = [];
    for (const p of s.parts) {
      await mp3(renderMidi(B.o, midi, { solo: p.ch }), join(dir, `ch${p.ch}.mp3`));
      // metrics on a 30 s excerpt from the channel's first note
      const first = firstNote(midi, p.ch);
      const ex = Object.fromEntries(keys.map((b) => [b, renderMidi(B[b], midi, { solo: p.ch, from: Math.max(0, first - 0.3), secs: EXCERPT })])) as Record<Key, Audio>;
      stems.push({ ch: p.ch, ...four(ex) });
    }
    results[s.id] = { id: s.id, seconds: Math.round(midi.duration), mix: mixM, stems };
    writeFileSync(resPath, JSON.stringify(results, null, 1));
    console.log(`${k + 1}/${sel.length} ${s.id} ${(midi.duration / 60).toFixed(1)} min, tf ${mixM.tf}/${mixM.plus.tf}/${mixM.sf.tf}, ${((performance.now() - t0) / 1000).toFixed(0)} s`);
  }
}

function firstNote(midi: BasicMIDI, ch: number): number {
  let best = Infinity;
  for (const t of midi.tracks) for (const e of t.events) {
    const st = e.statusByte;
    if ((st & 0xf0) === 0x90 && (st & 15) === ch && e.data[1] > 0 && e.ticks < best) best = e.ticks;
  }
  return Number.isFinite(best) ? midi.midiTicksToSeconds(best) : 0;
}

/** The note that plays each wave: drum kit key first (Standard), else a melodic region at the wave's unity key. */
export function waveNotes() {
  type Ins = { drums: boolean; bank: number[] | number; program: number; name: string; regions: { key: number[]; wave: number }[] };
  const st = JSON.parse(readFileSync(join(ROOT, "assets/gm/structure.json"), "utf8")) as { instruments: Ins[]; waves: { wsmp: number[] }[] };
  const bankNo = (b: Ins["bank"]) => (Array.isArray(b) ? b[0] : b);
  return st.waves.map((w, i) => {
    const users = st.instruments.flatMap((ins) => ins.regions.filter((r) => r.wave === i).map((r) => ({ ins, r })));
    users.sort((a, b) => +b.ins.drums - +a.ins.drums || bankNo(a.ins.bank) - bankNo(b.ins.bank) || a.ins.program - b.ins.program);
    const u = users[0];
    if (!u) return null;
    const unity = w.wsmp[0];
    const key = u.ins.drums ? u.r.key[0] : Math.min(u.r.key[1], Math.max(u.r.key[0], unity));
    return { w: i, drums: u.ins.drums, bank: bankNo(u.ins.bank), program: u.ins.program, key, name: u.ins.name };
  });
}

export const NOTE = { velocity: 100, hold: 1.6, total: 2.6 };
/** One note through `bank` (channel 9 for kits). */
export function renderNote(bank: BasicSoundBank, n: { drums: boolean; bank: number; program: number; key: number }): Audio {
  const synth = new SpessaSynthProcessor(RATE, { effectsEnabled: true });
  synth.soundBankManager.addSoundBank(bank, "main");
  const ch = n.drums ? 9 : 0;
  synth.controllerChange(ch, 0, n.bank);
  synth.programChange(ch, n.program);
  synth.noteOn(ch, n.key, NOTE.velocity);
  const total = Math.round(NOTE.total * RATE), off = Math.round(NOTE.hold * RATE);
  const L = new Float32Array(total), R = new Float32Array(total);
  for (let i = 0; i < total; i += Q) { if (i <= off && off < i + Q) synth.noteOff(ch, n.key); synth.process(L, R, i, Math.min(Q, total - i)); }
  return { rate: RATE, ch: [L, R] };
}

async function waves() {
  const o = SoundBankLoader.fromArrayBuffer(ab(env("GM_DLS")));
  const notes = waveNotes();
  mkdirSync(join(OUT, "waves"), { recursive: true });
  for (const n of notes) if (n) await mp3(renderNote(o, n), join(OUT, "waves", `${String(n.w).padStart(3, "0")}.mp3`));
  writeFileSync(join(OUT, "waves.json"), JSON.stringify(notes));
  console.log(`${notes.filter(Boolean).length} waves → ${join(OUT, "waves")}`);
}

if (import.meta.main) {
  const [cmd] = process.argv.slice(2);
  const i = process.argv.indexOf("--only");
  const pa = process.argv.indexOf("--part");
  const mx = process.argv.indexOf("--max");
  if (cmd === "songs") await songs(i > 0 ? new RegExp(process.argv[i + 1]) : undefined, pa > 0 ? process.argv[pa + 1] : "0/1", mx > 0 ? +process.argv[mx + 1] : Infinity);
  else if (cmd === "waves") await waves();
  else console.log("usage: bun tools/spessa/render.ts songs [--only re] | waves");
}
