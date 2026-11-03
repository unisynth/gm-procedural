# Procedural synthesis of the GM instruments (2026-11)

Background for change `add-procedural-gm-bank`. For each GM category this note gives the published way to synthesise
it, the parameter priors to start the fit from (mode ratios, oscillator frequencies, formants), and the sources. The
fit tool (`gmfit analyze`) uses these priors for the initial parameters and as narrowed CMA-ES ranges. The search
then starts near a physically sensible point instead of searching blind.

## 0. What a GM wave actually has to contain (measured on Windows `gm.dls`)

- **A wave is attack plus a short loop. The long decay is not in the wave.** The note decay and release come from
  the DLS articulation (EG1 volume envelope). For example, the Piano 1 waves 289–298 are 4.8 k–13.7 k samples long:
  about 0.2–0.6 s of attack, then a loop of 155–2272 samples. The models therefore only need to reproduce the first
  ~0.5 s plus a steady-state loop, not a 10 s piano decay.
- **A loop of L samples is periodic with L, so its spectrum lies on the grid `rate/L`.** For example, the 1339-sample
  loop of wave 293 has a 16.5 Hz grid. Inharmonic partials (piano stretch, beating unisons) are already quantised to
  that grid in the original. Snapping our partials to `k·rate/L` (design D3) therefore reproduces the original's own
  constraint and loses nothing.
- Most loops hold several periods. That is how beating and chorus survive inside a loop. A model with 2–3 detuned
  unison components snapped to the grid can reproduce it.

## 1. Cross-cutting techniques

| Technique | Use | Key reference |
|---|---|---|
| Sines + noise (+ transients), SMS | Universal analysis to start from: harmonic tracks, plus a residual modelled as filtered noise, plus an attack transient | Serra & Smith, *Spectral Modeling Synthesis*, CMJ 14(4) 1990; sms-tools (UPF MTG) |
| Harmonic-plus-noise | The same split, with per-frame harmonic amplitudes and a filtered-noise envelope. It is the model behind our `additive` + `noise` layers | DDSP (Engel et al. 2020, arXiv 2001.04643) |
| Commuted synthesis | Fold the body/soundboard response into the excitation, which then drives a cheap string model. Our attack layer can be "excitation ∗ body", with the body given as a few resonances | Smith & Van Duyne, *Commuted Piano Synthesis*, ICMC 1995; Karjalainen & Välimäki (guitar) |
| Brightness follows amplitude | Brass and most wind spectra get brighter as they get louder. One parameter couples spectral tilt to the amplitude envelope | Risset trumpet analysis; FM brass (Chowning/Morrill), `dsprelated.com/freebooks/sasp/FM_Brass.html` |
| Modal synthesis | A struck object is a sum of damped sinusoids, each with a ratio, amplitude and decay. Strike position weights the modes | STK `Modal`/`ModalBar` (Cook & Scavone), Bank et al. *Modal-based real-time piano* (IEEE TASLP 2010) |
| PhISEM | Shaken or scraped sounds: random collision events (Poisson rate, energy decay) excite a few resonant filters | Cook, PhISEM; STK `Shakers`; Csound `phisem.c` |
| FM | A compact model for bright, evolving or bell-like spectra (DX7 electric piano, tubular bell, brass) | Chowning 1973; STK `Rhodey`, `TubeBell`, `FMVoices` |

Open-source code to consult (permissive licences): **STK** (Synthesis ToolKit, Cook & Scavone), **Mutable Instruments
Plaits/Elements/Rings** firmware (MIT; `plaits/dsp/engine/*`: analog bass drum, snare, hi-hat, modal, string, FM,
speech), and the **Csound** opcodes (`phisem.c`, `fof`, formant table). Gordon Reid's 63-part *Synth Secrets* (Sound
On Sound 1999–2004) walks through each instrument class below (parts 24–59).

## 2. Per GM category

Families named as in design D4. "Priors" are the starting values and ranges for `gmfit analyze`.

### Piano (GM 0–7: acoustic/bright/electric grand, honky-tonk, EP1, EP2, harpsichord, clavinet)
- **Acoustic piano.** Model it as stiff strings plus a hammer plus a soundboard.
  - Partials follow `f_n = n·f0·√(1+B n²)/√(1+B)`, with inharmonicity B ≈ 1e-4 in the bass, 3–5e-4 in the midrange
    and up to 1e-2 in the treble.
  - Double decay and beating come from 2–3 unison strings detuned by ~0.5–2 cents and coupled at the bridge.
  - The hammer behaves as a velocity-dependent low-pass impulse.
  - In our framework: `additive` with B, per-partial double decay and unison detune (or `modal`), plus a `noise`
    attack layer for the soundboard knock.
  - Sources: Smith & Van Duyne ICMC-95 (`ccrma.stanford.edu/~jos/pdf/svd95.pdf`); Bank thesis and TASLP 2010; Synth
    Secrets 42–45.
- **Honky-tonk.** Same as acoustic piano with a larger unison detune (≈ 10–25 cents).
- **Electric piano 1 (Rhodes).** A struck tine and tonebar, read through a nonlinear magnetic pickup. Model it as a
  modal tine (2 modes) followed by a soft-clip pickup curve, or as FM. STK `Rhodey` uses two summed FM pairs. Sources:
  DAFx-17 *Real-time Physical Model of a Wurlitzer and Rhodes* (paper 79).
- **Electric piano 2 (the DX7 "FM piano").** Use `fm` directly with 2–3 operators, modulator index decaying with
  time.
- **Harpsichord.** A quill pluck: a sharp, bright excitation driving `ks`, with a pluck position near the end (a bright
  comb), low loop damping and a release thump. Source: Perng, Smith & Rossing, DAFx-10.
- **Clavinet.** `ks` plus pickup-position comb filtering and a band-pass tone. Source: Aalto ICMC-11 clavinet
  emulation.

### Chromatic percussion (8–15: celesta, glockenspiel, music box, vibraphone, marimba, xylophone, tubular bells, dulcimer)
`modal`, with priors from tuned-bar acoustics and STK `ModalBar`:

| Instrument | Mode ratios (prior) | Notes |
|---|---|---|
| Marimba | 1 : 3.99 : 10.65 (+ fixed stick mode) | Undercut tuned 1:4:10; tube resonator boosts f0. STK preset 0 |
| Vibraphone | 1 : 2.01 : 3.9 : 14.37 | Long decay, tremolo (motor AM ≈ 4–7 Hz). STK preset 1 |
| Xylophone | 1 : 3 : 9 (shallow undercut) | Shorter decay, brighter |
| Glockenspiel / celesta | Uniform free bar 1 : 2.756 : 5.404 : 8.933 | Celesta is softer (felt hammer, resonator) |
| Tubular bells | 1 : 2.76 : 5.40 : 8.93 : 13.34 : 18.64 | Strike pitch is virtual, an octave below partial 4. FM `TubeBell` also works |
| Music box | Free-free tine, about 1 : 6.27 | Short tines, tiny attack click |
| Dulcimer | Struck strings: `ks` with a hammer excitation, 2–3 unison strings | |

Bells in general: the Risset bell is 11 partials with ratios 0.56, 0.563, 0.92, 0.923, 1.19, 1.70, 2.00, 2.74, 3.00,
3.74, 4.07. Each partial's duration falls with frequency, and the near-unison pairs beat. Sources: Synth Secrets 40;
Puckette, *Theory and Techniques*, §4 (Risset's bell); Euphonics 3.3; Rochester PHY103 bars & bells; hibberts.co.uk
(tubular bells).

### Organ (16–23: drawbar, percussive, rock, church, reed, accordion, harmonica, tango accordion)
- **Drawbar, percussive and rock organ.** Hammond additive: harmonic set {½ (sub), 1, 1.5 (×3 of the sub), 2, 3, 4,
  5, 6, 8} × f0, which are the footages 16′, 5⅓′, 8′, 4′, 2⅔′, 2′, 1⅗′, 1⅓′, 1′.
  - Key click is a short broadband transient (`noise`).
  - Percussion is a decaying 2nd or 3rd harmonic.
  - The Leslie is AM/FM vibrato.
  - With `additive` this takes 9 amplitudes plus a click, the cheapest instrument in the bank. Sources: Synth Secrets
    55–59; Hammond drawbar documentation.
- **Church / pipe organ.** `additive` with a stable spectrum and a small chiff `noise` at onset. A mixture stop is a
  set of extra harmonics.
- **Reed organ, accordion, harmonica.** Free reed: a bright, nearly sawtooth spectrum.
  - Accordion: 2–3 reeds detuned ("musette", a few cents) whose beating survives in the loop. Model it as `additive`
    with unison detune, plus a body/reed formant (`correct` handles that).
  - Harmonica: add breath `noise`.

### Guitar (24–31: nylon, steel, jazz, clean, muted, overdriven, distortion, harmonics)
- Extended Karplus-Strong (`ks`):
  - pick-position comb `1 − z^{−βN}`
  - level-dependent dynamics low-pass
  - fractional-delay all-pass tuning
  - dispersion all-pass for steel stiffness
  - loop damping filter
- **Acoustic body.** Commuted synthesis: excitation = pluck ∗ body impulse response. Model the body with 2–4
  resonances (≈ 100 Hz air mode, ≈ 200 Hz top-plate mode).
- **Muted.** High loop damping and a short decay.
- **Overdriven / distortion.** `ks` or `additive`, then a waveshaper. Alternatively fit the distorted steady spectrum
  directly with `additive` plus correction, since distortion of a periodic loop stays periodic.
- **Harmonics.** Pick position at ½ or ⅓ of the string, which removes the even modes or the multiples of 3.
- Sources: Jaffe & Smith 1983 (*Extensions of the Karplus-Strong Plucked-String Algorithm*); Välimäki, Pakarinen,
  Erkut & Karjalainen 2006 (*Discrete-time modelling of musical instruments*); Synth Secrets 28–30.

### Bass (32–39: acoustic, finger, pick, fretless, slap 1/2, synth bass 1/2)
- **Acoustic, finger, pick and fretless.** `ks` with a darker loop filter. Finger is a round, damped pluck; pick is
  brighter. Fretless has a smoother attack and a slight glide.
- **Slap.** A sharp thumb strike plus string-fret collision buzz: a transient `noise`/`drum` click on top of `ks`.
- **Synth bass.** `subtractive`: saw or square, a resonant low-pass whose cutoff envelope decays.
- Sources: Fraunhofer IDMT bass synthesis dataset (fingerstyle, slap-thumb, slap-pluck); MODO BASS notes.

### Strings and ensemble (40–55: violin, viola, cello, contrabass, tremolo, pizzicato, harp, timpani, ensembles, synth strings, choir aahs, voice oohs, synth voice, orchestra hit)
- **Bowed strings.** Helmholtz motion puts a sawtooth force on the bridge, and dense bridge and body resonances
  shape it. Model it as `formant` (a saw source through 4–5 body peaks) or as `additive` plus correction, with
  vibrato (≈ 5–6 Hz, ±10–30 cents) and bow `noise`. Sources: Synth Secrets 48–51; KTH *Acoustics for violin and guitar
  makers* ch. 8.
- **Tremolo strings.** AM of the bowed model at 6–12 Hz.
- **Pizzicato.** `ks` through the same body peaks.
- **Harp.** `ks` with a soft pluck, long decay and low dispersion.
- **Timpani.** `modal` with air-loaded membrane ratios relative to the principal (1,1) mode: 1 : 1.5 : 1.98 : 2.44.
  The (0,1) mode is heavily damped. Source: Synth Secrets 31; Rossing.
- **String ensemble 1 and 2.** The bowed model with 3–5 unison voices detuned a few cents, which gives chorus
  beating inside the loop.
- **Synth strings.** `subtractive` with saw/PWM and an ensemble chorus (Synth Secrets 46–47).
- **Choir aahs, voice oohs, synth voice.** `formant`, starting from the Csound vowel table. Vowel /a/ for aahs and /u/
  for oohs; soprano/alto/tenor/bass by pitch range:

  | Voice | Vowel | F1 | F2 | F3 | F4 | F5 (Hz) |
  |---|---|---:|---:|---:|---:|---:|
  | soprano | a | 800 | 1150 | 2900 | 3900 | 4950 |
  | alto | a | 800 | 1150 | 2800 | 3500 | 4950 |
  | tenor | a | 650 | 1080 | 2650 | 2900 | 3250 |
  | bass | a | 600 | 1040 | 2250 | 2450 | 2750 |
  | soprano | u | 325 | 700 | 2700 | 3800 | 4950 |
  | tenor | u | 350 | 600 | 2700 | 2900 | 3300 |

  The bandwidths and dB amplitudes come from the same table (csound.com/manual/misc/formants). A choir is an ensemble
  of voices with jitter and vibrato. Source: Synth Secrets 23.
- **Orchestra hit.** A chord stab from strings and brass. Use `modal`/`additive` with partials at chord-tone ratios
  (root, fifth, octave, third, …) plus a `noise` attack.

### Brass (56–63: trumpet, trombone, tuba, muted trumpet, French horn, brass section, synth brass 1/2)
- `additive` with brightness coupled to amplitude: the spectral tilt follows the envelope (Risset), and the attack has
  a short pitch scoop of ≈ −30 to −50 cents over 30–60 ms.
- Alternatively `fm`, with the modulation index proportional to the amplitude envelope (Morrill FM brass).
- **Muted trumpet.** Add a strong band-pass (mute resonance), which `correct` handles.
- **French horn.** Darker, with fewer upper harmonics.
- **Section.** Unison detune.
- **Synth brass.** `subtractive`: saw, with the low-pass cutoff following the amplitude envelope (Synth Secrets
  25–27).
- Sources: Risset; `dsprelated.com/freebooks/sasp/FM_Brass.html`; Dannenberg, *Synthesizing Trumpet Performances*.

### Reed (64–71: soprano/alto/tenor/baritone sax, oboe, English horn, bassoon, clarinet)
- **Clarinet.** A cylindrical bore closed at one end, so the lower register is dominated by odd harmonics.
  Mathews/Strong's spectrum-envelope method uses separate envelopes for the odd and even harmonics. Use `additive`
  with an odd/even gain split.
- **Saxophones, oboe, bassoon.** A conical bore gives all harmonics, plus formant-like peaks and anti-formants. Use
  `additive` plus correction, or `formant` with a saw source, plus a little breath `noise`.
- Sources: Synth Secrets 24; Euphonics 11.3; Dalmont (ASA 151) on the saxophone; Scavone's conical-bore papers.

### Pipe (72–79: piccolo, flute, recorder, pan flute, blown bottle, shakuhachi, whistle, ocarina)
- An air jet over an open tube gives all harmonics, falling off steeply, plus a large breath-noise component and a
  chiff at onset. Use `additive` with few harmonics plus `noise` (a band-pass around the harmonics, which is periodic
  inside the loop).
- **Pan flute, bottle, shakuhachi.** More noise and a breathy attack.
- **Whistle, ocarina.** Nearly a sine wave.
- Sources: Synth Secrets 52–54; Euphonics 11.8; STK `Flute`, `BlowBotl`.

### Synth lead, pad and effects (80–103)
- `subtractive` covers almost everything: square, saw, calliope, chiff, charang, voice, fifths and bass+lead leads;
  new age, warm, polysynth, choir, bowed, metallic, halo and sweep pads.
  - Supersaw and unison come from detune inside the loop.
  - Sweeps last longer than the wave, so the wave holds an early snapshot and the DLS articulation and LFO do the
    rest.
- **Effects (rain, soundtrack, crystal, atmosphere, brightness, goblin, echoes, sci-fi).** Mixes of `noise`, `fm`
  bells and `subtractive`, fitted per wave.

### Ethnic (104–111: sitar, banjo, shamisen, koto, kalimba, bagpipe, fiddle, shanai)
- **Sitar.** `ks` plus the jawari bridge buzz: a one-sided curved-bridge nonlinearity that keeps moving energy upwards
  (a "shimmer"). Approximate it with a time-varying brightness or a soft clip in the loop. Sources: Euphonics 12.3;
  the sccode DWG sitar model.
- **Banjo.** `ks` with a membrane body: bright, quick decay, a strong 2–5 kHz resonance.
- **Shamisen.** `ks` plus sawari buzz, like the sitar but lighter.
- **Koto.** `ks` with a soft pluck and long decay.
- **Kalimba.** `modal` with 2 dominant modes, f1/f0 ≈ 5.9–6.8, plus an attack click (ASA 155, Chapman).
- **Bagpipe.** Chanter (a bright reed, `additive` with a strong high-mid formant) plus a drone at a lower octave.
- **Fiddle.** Same as violin.
- **Shanai.** Double reed, like the oboe.

### Percussive (112–119: tinkle bell, agogo, steel drums, woodblock, taiko, melodic tom, synth drum, reverse cymbal)
- **Agogo.** `modal` with STK preset 2: 1 : 4.08 : 6.669.
- **Steel drums.** `modal` with harmonically tuned note modes: fundamental, octave, octave + fifth (×3), two octaves
  (×4). Sources: Rossing (bass steelpans); Stockholm Steelband tuning notes.
- **Woodblock.** STK `ModalBar` Wood1/Wood2 presets. High damping, 2–3 modes.
- **Taiko, melodic tom.** `drum` with membrane ratios (1 : 1.593 : 2.136 : 2.296 : 2.653 : 2.917) and a pitch drop of
  ≈ 5–15 % over the first 50–100 ms. A tom is the `drum` body sine plus 1–2 inharmonic modes.
- **Synth drum.** The `drum` family directly.
- **Reverse cymbal.** A `noise`/`metal` cymbal with a rising amplitude envelope.

### Sound effects (120–127: fret noise, breath, seashore, bird, telephone, helicopter, applause, gunshot)
- **Fret noise.** A short band-pass `noise` squeak with a pitch sweep.
- **Breath.** Filtered `noise`.
- **Seashore.** `noise` with a slow amplitude swell. The wave holds one swell segment.
- **Bird tweet.** `fm` or `additive` with fast frequency chirps, i.e. a pitch envelope with an LFO.
- **Telephone ring.** Two tones alternated at the bell-hammer rate, or an inharmonic bell from `modal`.
- **Helicopter.** `noise` with AM at the rotor rate (≈ 10–20 Hz) plus a low thump.
- **Applause.** PhISEM: dense random claps, each a short band-pass noise burst.
- **Gunshot.** A broadband `noise` burst with a fast decay and a low-pass tail.

### Drum kits (91 waves: Standard, Room, Power, Electronic, TR-808, Jazz, Brush, Orchestra, SFX)
- **Kick.**
  - Acoustic: `drum` with a body sine dropping from ≈ 2–3× f_end to f_end ≈ 45–70 Hz within 20–60 ms, a beater click
    (short `noise` high-pass), and optionally a 2nd membrane mode.
  - TR-808: a bridged-T resonator, i.e. a decaying sine with a small pitch drop, ≈ 50–60 Hz and long.
  - Sources: Synth Secrets 33–34; Plaits `bass_drum_engine` (analog) and `synthetic_bass_drum`.
- **Snare.** Two shell/air modes, ≈ 180 Hz and ≈ 330 Hz (the TR-909 uses two sines with different decays), plus snare
  wires: band-passed or high-passed `noise` with its own decay. Sources: Synth Secrets 35–36; Plaits
  `snare_drum_engine`.
- **Hi-hats (closed, pedal, open) and cymbals.** `metal`.
  - TR-808 metallic noise: 6 square oscillators at 205.3, 304.4, 369.6, 522.7, 540, 800 Hz (Plaits normalises this
    to ratios 1, 1.304, 1.466, 1.787, 1.932, 2.536 at 2·f0 ≈ 414 Hz), summed, then band-passed (≈ 7–10 kHz for the
    hat), high-passed and enveloped. Closed hat ≈ 30–60 ms decay, open hat ≈ 300–800 ms.
  - Ride, crash and splash: denser inharmonic clusters. Ring-modulated square×saw pairs (Plaits' second hi-hat:
    200×7530, 510×8075, 730×10500 Hz) or FM, plus a band-limited noise "fog", and a bell tone for the ride.
  - Sources: Synth Secrets 37–39; Plaits `hi_hat_engine`.
- **Toms.** `drum` with membrane ratios and a pitch drop; floor and high toms differ in f0 and decay.
- **Cowbell.** `metal` with 2 squares at 540 and 800 Hz (808 service manual; SOS gives 587/845), band-passed, with a
  short envelope. Source: Synth Secrets 41.
- **Claves, woodblock.** `modal` with a single high mode ≈ 2.5 kHz and fast decay (Synth Secrets 41).
- **Shakers (maracas, cabasa, tambourine jingles, shaker).** PhISEM: Poisson collision events into 1–3 resonances.
  Tambourine = a membrane hit plus jingle resonances (STK `Shakers` presets).
- **Congas, bongos, timbales, cuíca, guiro, whistle, vibraslap, triangle, castanets, ….** `drum`/`modal`/`noise`
  combinations with the priors above. The triangle is a high `modal` with a long decay and inharmonic rod modes.
- **Brush kit.** Snare = swept `noise` with a slow attack.
- **Electronic and 808 kits.** `drum`/`metal` with analog parameters. These are the easiest and are expected to
  match closely.

## 3. Consequences for the design

1. **Add 3 families to design D4:**
   - `fm`: 2–4 operators with index envelopes. Covers EP2, tubular bell, FM brass, bird tweet and some synth pads.
   - `metal`: N square or ring-modulated oscillators at inharmonic ratios, then band-pass and high-pass filtering.
     Covers 808 hats and cymbals, cowbell and early-drum-machine cymbals.
   - `shaker`: PhISEM stochastic collision events into resonators. Covers maracas, cabasa, tambourine, sleigh bells
     and applause.
2. **`ks` is the extended Karplus-Strong model:** pick-position comb, dynamics low-pass, all-pass tuning,
   dispersion, an optional buzz nonlinearity (sitar/shamisen), and commuted body resonances in the excitation.
3. **`additive` gains:** an inharmonicity term B, unison detune with 2–3 voices, per-partial double decay,
   brightness coupled to amplitude, an odd/even split gain, and a drawbar mode (9 Hammond partials).
4. **`formant` is a general source-filter family:** saw/pulse/noise through ≤ 5 resonances. It covers vowels (Csound
   table priors), the bowed-string body and the sax/oboe body.
5. **`gmfit analyze` priors table:** the mode-ratio sets (marimba, vibraphone, xylophone, free bar, tubular bell,
   Risset bell, membrane, timpani, kalimba, steel pan, agogo, woodblock), the 808 hat/cowbell frequencies, the vowel
   formants and the Hammond footages. Each is chosen from the GM program or drum key that uses the wave.
6. **Expected difficulty** (to check in the pilot):
   - easy: 808/electronic kit, organs, synth leads/pads, flutes/whistle, mallets
   - medium: bass, guitars, brass, reeds, timpani, toms, cymbals
   - hard: acoustic piano attack, choir, nylon guitar, sax, applause, seashore
