// Live engine of the site (bundled to site/assets/live.js by tools/site/build.ts): SpessaSynth in an AudioWorklet
// playing MIDI through the banks that may be published or built: gen-gm and gen-plus generated in the browser by
// gmgen.wasm, gmplus.sf2 from the bucket. gm.dls is never loaded here (its renders are plain audio files).
import { Sequencer, WorkletSynthesizer } from "spessasynth_lib";

export type LiveBank = "s" | "g" | "p";
export type Progress = (state: { bank: LiveBank; label: string; frac: number }) => void;
export const BUCKET = "https://storage.googleapis.com/gm-procedural/site/";
const WORKLET = new URL("./spessasynth_processor.min.js", import.meta.url).href;
const WORKER = new URL("./gmgen-worker.js", import.meta.url).href;
// gmgen.wasm is a binary, so it lives in the bucket; the query is its content hash (set at bundle time) to bust caches
declare const GMGEN_WASM_HASH: string;
const GMGEN_WASM = `${BUCKET}gmgen.wasm?v=${GMGEN_WASM_HASH}`;

const buffers: Partial<Record<LiveBank, Promise<ArrayBuffer>>> = {};
export const built: Partial<Record<LiveBank, number>> = {}; // ms to build (procedural banks)

function generate(set: 0 | 1, bank: LiveBank, progress?: Progress): Promise<ArrayBuffer> {
  return new Promise((resolve, reject) => {
    const w = new Worker(WORKER, { type: "module" });
    const name = set ? "gen-plus" : "gen-gm";
    w.onmessage = ({ data }) => {
      if (data.error) { w.terminate(); reject(new Error(data.error)); }
      else if (data.done) { built[bank] = data.ms; w.terminate(); resolve(data.done); }
      else progress?.({ bank, label: `building ${name} in your browser · wave ${data.wave}/${data.waves}`, frac: data.progress });
    };
    w.onerror = (e) => { w.terminate(); reject(e); };
    w.postMessage({ set, wasm: GMGEN_WASM });
  });
}

async function download(url: string, bank: LiveBank, label: string, progress?: Progress): Promise<ArrayBuffer> {
  const r = await fetch(url);
  if (!r.ok || !r.body) throw new Error(`${url}: ${r.status}`);
  const total = +(r.headers.get("content-length") ?? 0);
  const chunks: Uint8Array[] = []; let got = 0;
  const rd = r.body.getReader();
  for (;;) {
    const { done, value } = await rd.read();
    if (done) break;
    chunks.push(value); got += value.length;
    progress?.({ bank, label: `${label} · ${(got / 1048576).toFixed(1)} MB`, frac: total ? got / total : 0 });
  }
  const out = new Uint8Array(got); let o = 0;
  for (const c of chunks) { out.set(c, o); o += c.length; }
  return out.buffer;
}

/** The bank's DLS/SF2 bytes, fetched or generated once per page. */
export function bankBytes(bank: LiveBank, progress?: Progress): Promise<ArrayBuffer> {
  return (buffers[bank] ??= bank === "s"
    ? download(`${BUCKET}banks/gmplus.sf2`, bank, "loading gmplus.sf2", progress)
    : generate(bank === "g" ? 0 : 1, bank, progress).catch(() => download(`${BUCKET}banks/${bank === "g" ? "gen-gm" : "gen-plus"}.dls`, bank, "loading the prebuilt bank", progress)));
}

export class Live {
  ctx: AudioContext;
  out: AudioNode;
  synth?: WorkletSynthesizer;
  seq?: Sequencer;
  bank?: LiveBank;
  private ready?: Promise<void>;
  constructor(ctx: AudioContext, out: AudioNode) { this.ctx = ctx; this.out = out; }

  private async init() {
    await this.ctx.audioWorklet.addModule(WORKLET);
    this.synth = new WorkletSynthesizer(this.ctx);
    this.synth.connect(this.out);
    await this.synth.isReady;
  }
  /** Loads (and if needed builds) `bank` into the synth; the same id replaces the previous bank. */
  async use(bank: LiveBank, progress?: Progress) {
    await (this.ready ??= this.init());
    if (this.bank === bank) return;
    const bytes = await bankBytes(bank, progress);
    const sm = this.synth!.soundBankManager;
    await sm.addSoundBank(bytes.slice(0), "main");
    this.bank = bank;
  }
  /** A song: the sequencer is created once; a new MIDI replaces the song list. */
  load(midi: ArrayBuffer) {
    this.seq ??= new Sequencer(this.synth!, { skipToFirstNoteOn: false } as never);
    this.seq.skipToFirstNoteOn = false;
    this.seq.loopCount = 0;
    this.seq.loadNewSongList([{ binary: midi, fileName: "song.mid" }] as never);
    this.seq.pause();
  }
  solo(ch: number | null) {
    this.synth?.midiChannels.forEach((c, i) => c.setSystemParameter("isMuted", ch !== null && i !== ch));
  }
  /** One note (sounds page): bank select + program on channel 0, or channel 9 for kits. */
  note(n: { drums: boolean; bank: number; program: number; key: number }, hold: number) {
    const s = this.synth!, ch = n.drums ? 9 : 0;
    s.stopAll(true);
    s.controllerChange(ch, 0 as never, n.bank);
    s.programChange(ch, n.program);
    s.noteOn(ch, n.key, 100);
    const t = setTimeout(() => s.noteOff(ch, n.key), hold * 1000);
    return () => { clearTimeout(t); s.noteOff(ch, n.key); };
  }
  stop() { this.seq?.pause(); this.synth?.stopAll(true); }
}
