"""Measure what the gmfit score averages away, for one GM wave (orig vs gen).

    uv run -q --with numpy python .pi/skills/gmfit/measure.py 405        # after `gmfit render 405`

Reads build/gm/NNN/{orig,gen}.wav (mono 16-bit, `played` = attack then loop repeated, 1 s) and the loop from
assets/gm/structure.json (waves[N].wsmp = [unity, fine, gain, loop start, loop length]). Prints one line per check:
orig, gen, OK/OFF and the fix to try. Checks that need several periods in the loop (cluster width, gap floor) say
n/a for single-period loops (organ, bass): their spectrum is exact harmonics by construction.
Thresholds follow design D5 of add-wav-resynthesis (provisional until calibrated).
"""
import json
import sys
import wave
from pathlib import Path

import numpy as np

root = Path(__file__).resolve().parents[3]
w = int(sys.argv[1])
d = root / "build/gm" / f"{w:03d}"


def rd(p):
    f = wave.open(str(p))
    return np.frombuffer(f.readframes(f.getnframes()), dtype=np.int16).astype(float) / 32768, f.getframerate()


a, sr = rd(d / "orig.wav")
b, _ = rd(d / "gen.wav")
info = json.load(open(root / "assets/gm/structure.json"))["waves"][w]
ws = info["wsmp"]
ls, ll = (ws[3], ws[4]) if len(ws) >= 5 else (0, 0)
looped = ll > 0
rows = []


def row(name, o, g, ok, fix, unit=""):
    rows.append(f"{name:22s} orig {o:>9}{unit}  gen {g:>9}{unit}  {'OK ' if ok else 'OFF'}  {'' if ok else fix}")


def db(x):
    return 10 * np.log10(x + 1e-30)


# f0 and harmonic spacing on the loop grid (comb energy: the autocorrelation pitch can be one period/loop off).
seg = (ls, ls + ll * max(1, 8192 // ll)) if looped else (0, min(len(a), 8192))
n = seg[1] - seg[0]
A = np.abs(np.fft.rfft(a[seg[0]:seg[1]])) ** 2
B = np.abs(np.fft.rfft(b[seg[0]:seg[1]])) ** 2
reps = n // ll if looped else 1
f = np.arange(len(A)) * sr / n
grid = (np.arange(len(A)) % reps == 0)
cands = range(1, 400) if looped else range(int(40 * n / sr), int(2000 * n / sr))
step = reps if looped else 1
per = max(cands, key=lambda p: sum(A[k * p * step] for k in range(1, 13) if k * p * step < len(A))) * step
f0 = per * sr / n
pitched = A[[k * per for k in range(1, 8) if k * per < len(A)]].sum() > 0.3 * A[1:].sum()
print(f"wave {w}: rate {sr}, loop {ls}+{ll}" if looped else f"wave {w}: rate {sr}, no loop", f"f0 {f0:.1f} Hz" if pitched else "unpitched")


# Buzz: per-period peak/mean of the 2.5-10 kHz band (score::buzz_band; also the `buzz` flag).
def buzz(x, lo, hi):
    x = x[2000:20000]
    X = np.fft.rfft(x)
    ff = np.fft.rfftfreq(len(x), 1 / sr)
    X[(ff < lo) | (ff > hi)] = 0
    y = np.fft.irfft(X, len(x))
    e = np.convolve(y ** 2, np.ones(11) / 11, "valid")
    p = int(round(sr / f0)) if pitched else 100
    e = e[: len(e) // p * p].reshape(-1, p)
    return np.mean(e.max(1) / (e.mean(1) + 1e-30))


for lo, hi in ((1500, 2500), (2500, 4000), (4000, 7000), (7000, min(10000, sr * 0.45))) if pitched and looped else ():
    bo, bg = buzz(a, lo, hi), buzz(b, lo, hi)
    row(f"buzz {lo}-{int(hi)}", f"{bo:.2f}", f"{bg:.2f}", bg <= 1.25 * bo, "phase_seed (random upper phases)")

if not (pitched and looped):
    rows.append("buzz                      n/a (needs a pitched loop)")
# Periodicity per band (pitched loops): best normalised autocorrelation at one period ±4 % over 50 ms frames.
# 1 = pitched, ~0 = noise. Lower than the original = hiss ("shhh"); higher = too clean/synthetic (405 round 10).
def periodicity(x, lo, hi):
    X = np.fft.rfft(x)
    ff = np.fft.rfftfreq(len(x), 1 / sr)
    X[(ff < lo) | (ff > hi)] = 0
    y = np.fft.irfft(X, len(x))
    p0 = sr / f0
    vals = []
    for t0 in range(int(0.1 * sr), len(y) - int(0.1 * sr), int(0.05 * sr)):
        seg = y[t0:t0 + int(0.05 * sr)]
        best = 0.0
        for lag in range(int(p0 * 0.96), int(p0 * 1.04) + 1):
            u, v = seg, y[t0 + lag:t0 + lag + len(seg)]
            if len(v) < len(u):
                break
            best = max(best, np.dot(u, v) / np.sqrt(np.dot(u, u) * np.dot(v, v) + 1e-20))
        vals.append(best)
    return float(np.mean(vals)) if vals else 0.0


if pitched and looped:
    for lo, hi in ((1500, 3000), (3000, 6000), (6000, min(9500, sr * 0.45))):
        po, pg = periodicity(a, lo, hi), periodicity(b, lo, hi)
        row(f"periodicity {lo}-{int(hi)}", f"{po:.2f}", f"{pg:.2f}", abs(pg - po) <= 0.07,
            "hiss: noise too strong (gap_noise bands)" if pg < po else "too clean: missing noise/smear")

# Loudness wobble over the played second after the attack (looped only; a one-shot's contour is the attack grid).
e = lambda x: db(np.convolve(x[int(0.1 * sr):] ** 2, np.ones(int(0.01 * sr)) / int(0.01 * sr), "valid"))
so, sg = (e(a).std(), e(b).std()) if looped else (0.0, 0.0)
looped and row("loudness sd", f"{so:.2f}", f"{sg:.2f}", abs(sg - so) <= max(0.25 * so, 0.2), "am rows / side rows / env", " dB")

if pitched and looped and per // reps >= 10:  # ≥ 10 periods per loop, so the loop grid has bins between harmonics
    # Cluster width (cents) on harmonics 1-12, and the mid-gap floor per band.
    def width(P, k):
        c = k * per
        r = np.arange(max(1, c - per // 2), min(len(P), c + per // 2 + 1))
        p = P[r] / P[r].sum()
        cents = 1200 * np.log2(r / c)
        return np.sqrt((p * cents ** 2).sum())

    wo = np.mean([width(A, k) for k in range(1, 13) if (k + 1) * per < len(A)])
    wg = np.mean([width(B, k) for k in range(1, 13) if (k + 1) * per < len(B)])
    row("cluster width h1-12", f"{wo:.1f}", f"{wg:.1f}", abs(wg - wo) <= 0.3 * wo + 1, "vibrato/vib, side rows", " c")
    idx = np.arange(len(A))
    off = np.minimum(idx % per, per - idx % per)
    far = (off >= per // 5) & grid
    tot = A[1:].sum()
    for lo, hi in ((120, 1000), (1000, 2500), (2500, 6000), (6000, sr * 0.45)):
        m = (f >= lo) & (f < hi) & far
        if m.sum() == 0:
            continue
        go, gg = db(A[m].mean() / tot), db(B[m].mean() / tot)
        row(f"gap floor {lo}-{int(hi)}", f"{go:.1f}", f"{gg:.1f}", go < -70 or abs(gg - go) <= 6, "gap_noise", " dB")
else:
    rows.append("cluster width / gap floor  n/a (unpitched, unlooped, or < 10 periods per loop)")

# Rumble below 0.7·f0, and the top octave 0.35-0.5·rate (dB re total).
tot = A[1:].sum()
if pitched:
    m = (f > 0) & (f < 0.7 * f0)
    ro, rg = db(A[m].sum() / tot), db(B[m].sum() / tot)
    row("rumble < 0.7 f0", f"{max(ro, -200):.1f}", f"{max(rg, -200):.1f}", rg <= ro + 6, "noise hp", " dB")
m = f >= 0.35 * sr
to, tg = db(A[m].sum() / tot), db(B[m].sum() / tot)
row("top 0.35-0.5 rate", f"{to:.1f}", f"{tg:.1f}", to < -60 or abs(tg - to) <= 6, "noise lp, harmonic tail", " dB")

# Attack: level per 50 ms x 4 bands over the first 0.3 s (tf grid, coarse).
off_cells = []
for t0 in range(0, int(0.3 * sr), int(0.05 * sr)):
    sa, sb = a[t0:t0 + 1024], b[t0:t0 + 1024]
    if len(sa) < 1024:
        sa, sb = np.pad(sa, (0, 1024 - len(sa))), np.pad(sb, (0, 1024 - len(sb)))
        if not sa.any():
            break
    PA, PB = np.abs(np.fft.rfft(sa * np.hanning(1024))) ** 2, np.abs(np.fft.rfft(sb * np.hanning(1024))) ** 2
    ff = np.fft.rfftfreq(1024, 1 / sr)
    for lo, hi in ((0, 500), (500, 2000), (2000, 6000), (6000, sr / 2)):
        m = (ff >= lo) & (ff < hi)
        lo_, lg = db(PA[m].sum()), db(PB[m].sum())
        if lo_ > db(PA.sum()) - 40 and abs(lg - lo_) > 3:
            off_cells.append(f"{t0 * 1000 // sr}ms/{lo}-{int(hi)}Hz {lg - lo_:+.0f}")
rows.append(f"attack grid (±3 dB)      {'OK ' if not off_cells else 'OFF'}  " + ("" if not off_cells else "tf / env: " + ", ".join(off_cells[:8])))

print("\n".join(rows))
