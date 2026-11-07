import { describe, expect, test } from "bun:test";
import { dist } from "./dist.ts";
import { pairsOf, rightPass, waveData } from "./dlsname.ts";
import { eventText, parse, parts, solo, trim } from "./smf.ts";

// SMF: format 1, 480 ppq, tempo track (120 → 60 bpm at beat 1) + a running-status track on ch1 and ch10.
function smf(): Uint8Array {
  const trk = (b: number[]) => [0x4d, 0x54, 0x72, 0x6b, 0, 0, b.length >> 8, b.length & 255, ...b];
  const t0 = trk([0, 0xff, 0x51, 3, 0x07, 0xa1, 0x20, 0x83, 0x60, 0xff, 0x51, 3, 0x0f, 0x42, 0x40, 0, 0xff, 0x2f, 0]);
  const t1 = trk([0, 0xc0, 5, 0, 0xf0, 2, 1, 0xf7, 0, 0x90, 60, 100, 0x83, 0x60, 62, 100, 0x83, 0x60, 60, 0, 0, 0x99, 36, 90, 0, 0xff, 0x2f, 0]);
  return new Uint8Array([0x4d, 0x54, 0x68, 0x64, 0, 0, 0, 6, 0, 1, 0, 2, 0x01, 0xe0, ...t0, ...t1]);
}

describe("smf", () => {
  const s = parse(smf(), 1000);
  test("tempo map and running status", () => {
    // beat 0 at 0 s (120 bpm: 0.5 s/beat), beat 1 at 0.5 s, then 60 bpm: beat 2 at 1.5 s
    expect(s.events.map((e) => [e.frame, ...e.bytes])).toEqual([
      [0, 0xc0, 5], [0, 0x90, 60, 100], [500, 0x90, 62, 100], [1500, 0x90, 60, 0], [1500, 0x99, 36, 90]]);
    expect(s.seconds).toBeCloseTo(1.5);
  });
  test("parts per channel with programs and keys", () => {
    expect(parts(s.events)).toEqual([
      { ch: 0, drums: false, notes: 2, programs: { 5: 2 }, keys: { 60: 1, 62: 1 } },
      { ch: 9, drums: true, notes: 1, programs: { 0: 1 }, keys: { 36: 1 } }]);
  });
  test("solo keeps setup, drops other channels' notes", () => {
    expect(solo(s.events, 9).map((e) => e.bytes[0])).toEqual([0xc0, 0x99]);
  });
  test("trim moves the first note to the lead", () => {
    const t = trim({ events: s.events.map((e) => ({ ...e, frame: e.frame + 5000 })), frames: 6500, seconds: 6.5 }, 1000, 0.5);
    expect(t.events[1].frame).toBe(500);
    expect(eventText(t.events, 10).split("\n")[0]).toBe("len 10");
  });
});

describe("dist", () => {
  const tone = (amp: number, f: number) => ({ rate: 44100, ch: [Float32Array.from({ length: 44100 }, (_, i) => amp * Math.sin((2 * Math.PI * f * i) / 44100))] });
  test("identical → 0, gain removed", () => {
    expect(dist(tone(0.5, 440), tone(0.5, 440)).tf).toBeCloseTo(0, 6);
    const d = dist(tone(0.25, 440), tone(0.5, 440));
    expect(d.gain).toBeCloseTo(-6.02, 1);
    expect(d.tf).toBeCloseTo(0, 6);
  });
  test("different timbre is far", () => {
    expect(dist(tone(0.5, 2000), tone(0.5, 440)).tf).toBeGreaterThan(10);
  });
});

describe("nameWaves", () => {
  test("names every wave, cues point at the waves, rest unchanged", async () => {
    const { nameWaves } = await import("./dlsname.ts");
    const c = (id: string, b: Buffer) => { const h = Buffer.alloc(8); h.write(id, "ascii"); h.writeUInt32LE(b.length, 4); return Buffer.concat([h, b, Buffer.alloc(b.length & 1)]); };
    const l = (t: string, b: Buffer) => c("LIST", Buffer.concat([Buffer.from(t), b]));
    const wave = (n: number) => l("wave", c("data", Buffer.alloc(n, 7)));
    const pt = Buffer.alloc(16); pt.writeUInt32LE(8); pt.writeUInt32LE(2, 4); pt.writeUInt32LE(0, 8); pt.writeUInt32LE(wave(3).length, 12);
    const src = c("RIFF", Buffer.concat([Buffer.from("DLS "), c("colh", Buffer.alloc(4)), c("ptbl", pt), l("wvpl", Buffer.concat([wave(3), wave(4)]))]));
    const out = nameWaves(src);
    expect(out.readUInt32LE(4)).toBe(out.length - 8);
    const w = out.indexOf("wvpl") + 4, p = out.indexOf("ptbl");
    for (const [k, name] of [[0, "w000"], [1, "w001"]] as const) {
      const at = w + out.readUInt32LE(p + 16 + 4 * k);
      expect(out.toString("ascii", at + 8, at + 12)).toBe("wave");
      expect(out.indexOf(name, at)).toBeLessThan(at + 8 + out.readUInt32LE(at + 4));
    }
    expect(out.subarray(12, 24).equals(src.subarray(12, 24))).toBe(true);
  });
});

// Stereo pairs (change add-gmplus-bank D7): the right-channel pass swaps each paired left wave's PCM for its partner's.
describe("stereo pairs", () => {
  const u32 = (n: number) => { const b = Buffer.alloc(4); b.writeUInt32LE(n); return b; };
  const chunk = (id: string, body: Buffer) => Buffer.concat([Buffer.from(id, "ascii"), u32(body.length), body]);
  const list = (t: string, body: Buffer) => chunk("LIST", Buffer.concat([Buffer.from(t, "ascii"), body]));
  const wave = (v: number) => list("wave", chunk("data", Buffer.from([v, v, v, v])));
  const bank = (pairs: Buffer | null) => {
    const body = Buffer.concat([Buffer.from("DLS ", "ascii"), list("wvpl", Buffer.concat([wave(1), wave(2), wave(3)])), pairs ?? Buffer.alloc(0)]);
    return chunk("RIFF", body);
  };
  test("pairs are read and the right pass copies the partner", () => {
    const d = bank(list("stpr", chunk("pair", Buffer.concat([u32(0), u32(2)]))));
    expect(pairsOf(d)).toEqual([[0, 2]]);
    const r = rightPass(d), w = waveData(r);
    expect([...r.subarray(w[0][0], w[0][0] + 4)]).toEqual([3, 3, 3, 3]);
    expect([...r.subarray(w[1][0], w[1][0] + 4)]).toEqual([2, 2, 2, 2]);
    expect(d.equals(bank(list("stpr", chunk("pair", Buffer.concat([u32(0), u32(2)])))))).toBe(true); // input untouched
  });
  test("a bank without pairs passes through unchanged", () => {
    const d = bank(null);
    expect(pairsOf(d)).toEqual([]);
    expect(rightPass(d).equals(d)).toBe(true);
  });
});
