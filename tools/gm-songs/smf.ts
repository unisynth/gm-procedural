// Standard MIDI File reader for the gm-songs comparison: channel events on an absolute sample clock.
export type Ev = { frame: number; bytes: number[] };
export type Song = { events: Ev[]; frames: number; seconds: number };

function vlq(d: Uint8Array, p: { i: number }): number {
  let v = 0;
  for (;;) {
    const b = d[p.i++];
    v = (v << 7) | (b & 0x7f);
    if (!(b & 0x80)) return v;
  }
}

/** Parses an SMF (format 0/1) into channel events timed in frames at `rate`. SysEx and meta are dropped (tempo kept). */
export function parse(data: Uint8Array, rate = 44100): Song {
  const str = (o: number) => String.fromCharCode(...data.subarray(o, o + 4));
  let o = str(0) === "RIFF" ? 20 : 0; // RMID wrapper: data chunk holds the SMF
  if (str(o) !== "MThd") throw new Error("not a MIDI file");
  const dv = new DataView(data.buffer, data.byteOffset);
  const ntrk = dv.getUint16(o + 10), div = dv.getUint16(o + 12);
  if (div & 0x8000) throw new Error("SMPTE time division not supported");
  o += 8 + dv.getUint32(o + 4);
  type Raw = { tick: number; seq: number; bytes?: number[]; tempo?: number };
  const raw: Raw[] = [];
  let seq = 0;
  for (let t = 0; t < ntrk && o + 8 <= data.length; t++) {
    const len = dv.getUint32(o + 4);
    if (str(o) !== "MTrk") { o += 8 + len; t--; continue; }
    const end = Math.min(o + 8 + len, data.length);
    const p = { i: o + 8 };
    let tick = 0, run = 0;
    while (p.i < end) {
      tick += vlq(data, p);
      let s = data[p.i];
      if (s & 0x80) p.i++; else s = run;
      if (s === 0xff) {
        const type = data[p.i++], n = vlq(data, p);
        if (type === 0x51 && n === 3) raw.push({ tick, seq: seq++, tempo: (data[p.i] << 16) | (data[p.i + 1] << 8) | data[p.i + 2] });
        p.i += n;
        if (type === 0x2f) break;
      } else if (s === 0xf0 || s === 0xf7) {
        const n = vlq(data, p); // not `p.i += vlq(..)`: that adds to the p.i read before vlq advanced it
        p.i += n;
      } else {
        run = s;
        const n = (s & 0xf0) === 0xc0 || (s & 0xf0) === 0xd0 ? 1 : 2;
        raw.push({ tick, seq: seq++, bytes: [s, ...data.subarray(p.i, p.i + n)] });
        p.i += n;
      }
    }
    o += 8 + len;
  }
  raw.sort((a, b) => a.tick - b.tick || a.seq - b.seq);
  let tempo = 500000, lastTick = 0, sec = 0;
  const events: Ev[] = [];
  for (const r of raw) {
    sec += ((r.tick - lastTick) * tempo) / 1e6 / div;
    lastTick = r.tick;
    if (r.tempo) tempo = r.tempo;
    else events.push({ frame: Math.round(sec * rate), bytes: r.bytes! });
  }
  return { events, frames: Math.round(sec * rate), seconds: sec };
}

/** Song moved so the first note sounds `lead` seconds in; earlier events (setup) are clamped to frame 0. */
export function trim(s: Song, rate = 44100, lead = 0.5): Song {
  const first = s.events.find((e) => (e.bytes[0] & 0xf0) === 0x90 && e.bytes[2]);
  const shift = first ? Math.max(0, first.frame - Math.round(lead * rate)) : 0;
  const events = s.events.map((e) => ({ frame: Math.max(0, e.frame - shift), bytes: e.bytes }));
  return { events, frames: s.frames - shift, seconds: (s.frames - shift) / rate };
}

/** Events with notes on other channels removed (controllers and programs stay, so the channel sounds as in the mix). */
export function solo(events: Ev[], ch: number): Ev[] {
  return events.filter((e) => (e.bytes[0] & 0xf0) !== 0x90 || (e.bytes[0] & 15) === ch);
}

/** `len` header + one "frame b0 b1 b2" line per event; length = song end + tail. */
export function eventText(events: Ev[], frames: number): string {
  return [`len ${frames}`, ...events.map((e) => `${e.frame} ${e.bytes.join(" ")}`)].join("\n") + "\n";
}

/** A sounding channel: notes per program (kit on the drum channel) and per key. */
export type Part = { ch: number; drums: boolean; notes: number; programs: Record<number, number>; keys: Record<number, number> };

/** Sounding channels (channel 10 = drums) with the programs active at note time and the keys played. */
export function parts(events: Ev[]): Part[] {
  const prog = new Array(16).fill(0), m = new Map<number, Part>();
  for (const e of events) {
    const [s, d1, d2] = e.bytes, ch = s & 15;
    if ((s & 0xf0) === 0xc0) prog[ch] = d1;
    if ((s & 0xf0) !== 0x90 || !d2) continue;
    const p = m.get(ch) ?? { ch, drums: ch === 9, notes: 0, programs: {}, keys: {} };
    p.notes++;
    p.programs[prog[ch]] = (p.programs[prog[ch]] ?? 0) + 1;
    p.keys[d1] = (p.keys[d1] ?? 0) + 1;
    m.set(ch, p);
  }
  return [...m.values()].sort((a, b) => a.ch - b.ch);
}
