// Builds a procedural bank in the browser: gmgen compiled to WebAssembly (crates/gmgen-wasm), wave by wave.
// message in: { set: 0 | 1, wasm: url }   (0 = gen-gm, the gm.dls layout at 22 kHz; 1 = gen-plus, the gmplus layout at 32 kHz;
// the wasm is served from the bucket, not from GitHub Pages: binaries live in Google Storage)
// messages out: { progress: 0..1, wave, waves } … then { done: ArrayBuffer (DLS), ms }
let wasm;
self.onmessage = async ({ data: { set, wasm: url } }) => {
  try {
    wasm ??= (await WebAssembly.instantiate(await (await fetch(url)).arrayBuffer(), {})).instance.exports;
    const t = performance.now();
    const n = wasm.begin(set);
    for (let i = 0; i < n; i++) {
      wasm.step(i);
      if (i % 5 === 4 || i === n - 1) self.postMessage({ progress: (i + 1) / n, wave: i + 1, waves: n });
    }
    const len = wasm.finish();
    const out = new Uint8Array(wasm.memory.buffer, wasm.out_ptr(), len).slice().buffer;
    wasm.release();
    self.postMessage({ done: out, ms: Math.round(performance.now() - t) }, [out]);
  } catch (e) {
    self.postMessage({ error: String(e) });
  }
};
