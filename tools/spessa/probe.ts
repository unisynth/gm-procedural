// Probe: can spessasynth_core play our banks? Renders N seconds of a MIDI through one bank, reports time, peak, RMS.
//   bun tools/spessa/probe.ts <bank.dls|sf2> <song.mid> <out.wav> [secs]
import { readFileSync, writeFileSync } from "node:fs";
import { audioToWav, BasicMIDI, SoundBankLoader, SpessaSynthProcessor, SpessaSynthSequencer } from "spessasynth_core";

const [bankPath, midPath, out, secsArg] = process.argv.slice(2);
const RATE = 44100, secs = +(secsArg ?? 20);
const ab = (p: string) => { const b = readFileSync(p); return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength); };
const t0 = performance.now();
const bank = SoundBankLoader.fromArrayBuffer(ab(bankPath));
const t1 = performance.now();
const synth = new SpessaSynthProcessor(RATE, { effectsEnabled: true });
synth.soundBankManager.addSoundBank(bank, "main");
await synth.processorInitialized;
const seq = new SpessaSynthSequencer(synth);
seq.loadNewSongList([BasicMIDI.fromArrayBuffer(ab(midPath))]);
seq.loopCount = 0;
seq.play();
const n = Math.min(Math.ceil(seq.duration * RATE), secs * RATE);
const L = new Float32Array(n), R = new Float32Array(n), Q = 128;
for (let i = 0; i < n; i += Q) { seq.processTick(); synth.process(L, R, i, Math.min(Q, n - i)); }
const t2 = performance.now();
let peak = 0, ss = 0; for (let i = 0; i < n; i++) { peak = Math.max(peak, Math.abs(L[i]), Math.abs(R[i])); ss += L[i] * L[i] + R[i] * R[i]; }
writeFileSync(out, new Uint8Array(audioToWav([L, R], RATE)));
console.log(JSON.stringify({ bank: bankPath.split("/").pop(), load_ms: Math.round(t1 - t0), render_ms: Math.round(t2 - t1), secs: n / RATE, duration: +seq.duration.toFixed(1), peak: +peak.toFixed(3), rms_db: +(10 * Math.log10(ss / (2 * n) + 1e-12)).toFixed(1) }));
