// AUMIDISynth identifies DLS waves by their INFO/INAM name: in a bank whose waves carry no name (the `dls::write`
// output) every region plays the first sample it loaded (Piano 1, wave 289). Apple's own bank names every wave.
// `nameWaves` adds `LIST INFO { INAM "wNNN" }` to each wave and rebuilds the `ptbl` cues; nothing else changes.

const u32 = (n: number) => { const b = Buffer.alloc(4); b.writeUInt32LE(n); return b; };
const chunk = (id: string, body: Buffer) => Buffer.concat([Buffer.from(id, "ascii"), u32(body.length), body, body.length & 1 ? Buffer.alloc(1) : Buffer.alloc(0)]);
const list = (type: string, body: Buffer) => chunk("LIST", Buffer.concat([Buffer.from(type, "ascii"), body]));

/** `RIFF DLS` with top-level `colh`, `lins`, …, `ptbl`, `LIST wvpl` → the same bank with named waves. */
export function nameWaves(d: Buffer): Buffer {
  const top: { id: string; type?: string; at: number; n: number }[] = [];
  for (let o = 12; o + 8 <= d.length; ) {
    const id = d.toString("ascii", o, o + 4), n = d.readUInt32LE(o + 4);
    top.push({ id, type: id === "LIST" ? d.toString("ascii", o + 8, o + 12) : undefined, at: o, n });
    o += 8 + n + (n & 1);
  }
  const wvpl = top.find((c) => c.type === "wvpl");
  if (!wvpl || !top.some((c) => c.id === "ptbl")) throw new Error("not a DLS bank (no ptbl / wvpl)");
  const waves: Buffer[] = [];
  for (let o = wvpl.at + 12, i = 0; o < wvpl.at + 8 + wvpl.n; i++) {
    const n = d.readUInt32LE(o + 4);
    const name = Buffer.from(`w${String(i).padStart(3, "0")}\0`, "ascii");
    waves.push(list("wave", Buffer.concat([d.subarray(o + 12, o + 8 + n), list("INFO", chunk("INAM", name))])));
    o += 8 + n + (n & 1);
  }
  let pos = 0;
  const cues = waves.map((w) => { const c = pos; pos += w.length; return c; });
  const ptbl = chunk("ptbl", Buffer.concat([u32(8), u32(cues.length), ...cues.map(u32)]));
  const body = top.map((c) => (c.id === "ptbl" ? ptbl : c === wvpl ? list("wvpl", Buffer.concat(waves)) : d.subarray(c.at, c.at + 8 + c.n + (c.n & 1))));
  return chunk("RIFF", Buffer.concat([Buffer.from("DLS ", "ascii"), ...body]));
}

/** Offsets of each wave's `data` payload in a DLS file (wvpl order). */
export function waveData(d: Buffer): [number, number][] {
  const out: [number, number][] = [];
  const walk = (o: number, end: number, inWave: boolean) => {
    while (o + 8 <= end) {
      const id = d.toString("ascii", o, o + 4), n = d.readUInt32LE(o + 4);
      if (id === "LIST") walk(o + 12, o + 8 + n, d.toString("ascii", o + 8, o + 12) === "wave");
      else if (inWave && id === "data") out.push([o + 8, n]);
      o += 8 + n + (n & 1);
    }
  };
  walk(12, d.length, false);
  return out;
}

/** Stereo pairs `(left, right)` of a DLS file's top-level `LIST 'stpr' { pair }` (change add-gmplus-bank D5). */
export function pairsOf(d: Buffer): [number, number][] {
  for (let o = 12; o + 8 <= d.length;) {
    const id = d.toString("ascii", o, o + 4), n = d.readUInt32LE(o + 4);
    if (id === "LIST" && d.toString("ascii", o + 8, o + 12) === "stpr") {
      const q = o + 12;
      if (d.toString("ascii", q, q + 4) !== "pair") return [];
      const m = d.readUInt32LE(q + 4), out: [number, number][] = [];
      for (let k = 0; k + 8 <= m; k += 8) out.push([d.readUInt32LE(q + 8 + k), d.readUInt32LE(q + 12 + k)]);
      return out;
    }
    o += 8 + n + (n & 1);
  }
  return [];
}
/** The right-channel pass of a paired bank (design D7): every paired left wave's PCM replaced by its partner's
 * (same length by the pair contract). AUMIDISynth plays mono waves, so stereo = left output of the bank as is +
 * right output of this copy. */
export function rightPass(d: Buffer): Buffer {
  const r = Buffer.from(d), w = waveData(d);
  for (const [l, p] of pairsOf(d)) {
    const [a, n] = w[l], [b, m] = w[p];
    if (n !== m) throw new Error(`pair ${l}/${p}: lengths ${n} ≠ ${m}`);
    d.copy(r, a, b, b + m);
  }
  return r;
}

