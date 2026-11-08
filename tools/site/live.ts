// Live engine of the site (bundled to site/assets/live.js by tools/site/build.ts): SpessaSynth in an AudioWorklet
// playing MIDI through the banks that may be published or built: gen-gm prebuilt from the bucket (checked against
// the bundled sha1, else built), gen-plus generated in the browser by gmgen.wasm and kept in the Cache API,
// gmplus.sf2 from the bucket. gm.dls is never loaded here (its renders are plain audio files).
import { Sequencer, WorkletSynthesizer } from "spessasynth_lib";

export type LiveBank = "s" | "g" | "p";
export type Progress = (state: { bank: LiveBank; label: string; frac: number }) => void;
export const BUCKET = "https://storage.googleapis.com/gm-procedural/site/";
const WORKLET = new URL("./spessasynth_processor.min.js", import.meta.url).href;
const WORKER = new URL("./gmgen-worker.js", import.meta.url).href;
// gmgen.wasm is a binary, so it lives in the bucket; the query is its content hash (set at bundle time) to bust caches
declare const GMGEN_WASM_HASH: string;
const GMGEN_WASM = `${BUCKET}gmgen.wasm?v=${GMGEN_WASM_HASH}`;
// sha1 of the prebuilt gen-gm (= gmgen.wasm output, tools/site/build.ts); the download is checked against it
declare const GEN_GM_SHA1: string;
const GEN_GM = `${BUCKET}banks/gen-gm.dls?v=${GEN_GM_SHA1.slice(0, 12)}`;
// built banks are kept in the Cache API, keyed by the wasm hash (a new generator invalidates them)
const CACHE = "gmgen";

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

/** gen-gm prebuilt (3.4 MB), accepted only when its sha1 matches the bundled one. */
async function prebuiltGm(progress?: Progress): Promise<ArrayBuffer> {
  const b = await download(GEN_GM, "g", "loading gen-gm", progress);
  const h = [...new Uint8Array(await crypto.subtle.digest("SHA-1", b))].map((x) => x.toString(16).padStart(2, "0")).join("");
  if (h !== GEN_GM_SHA1) throw new Error(`gen-gm.dls sha1 ${h}, expected ${GEN_GM_SHA1}`);
  return b;
}

/** A bank built once per wasm version: from the Cache API when present, else `make()` and store it. */
async function cached(name: string, make: () => Promise<ArrayBuffer>): Promise<ArrayBuffer> {
  const key = `${BUCKET}built/${name}.dls?v=${GMGEN_WASM_HASH}`;
  const cache = await caches.open(CACHE).catch(() => undefined); // no Cache API (insecure context): build every time
  const hit = await cache?.match(key);
  if (hit) return hit.arrayBuffer();
  const b = await make();
  if (cache) {
    for (const k of await cache.keys()) if (k.url.includes(`/built/${name}.dls`) && k.url !== key) await cache.delete(k);
    await cache.put(key, new Response(b.slice(0))).catch(() => {}); // quota: keep playing without caching
  }
  return b;
}

/** The bank's DLS/SF2 bytes, once per page: gmplus.sf2 downloaded; gen-gm prebuilt, else built (cached);
 *  gen-plus (29 MB prebuilt) built in the page and cached, else the prebuilt fallback. */
export function bankBytes(bank: LiveBank, progress?: Progress): Promise<ArrayBuffer> {
  const plusFallback = () => download(`${BUCKET}banks/gen-plus.dls`, bank, "loading the prebuilt bank", progress);
  return (buffers[bank] ??= bank === "s"
    ? download(`${BUCKET}banks/gmplus.sf2`, bank, "loading gmplus.sf2", progress)
    : bank === "g"
      ? prebuiltGm(progress).catch(() => cached("gen-gm", () => generate(0, bank, progress)))
      : cached("gen-plus", () => generate(1, bank, progress)).catch(plusFallback));
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
