# score.rs

Similarity score (design D6) of a generated wave vs the original, on the "played" signals (`PLAYED_SECS` 0.5 unroll).

- `d_spec`: log-mel at 3 STFT sizes (≥ 2 periods/512/2048, hop size/4, Hann), A-weighted per band (`a_weight`), floored at peak − `SPEC_RANGE` 50 dB; frames centred from t=0, first 50 ms count double.
- `d_onset`: same comparison over frames starting in the first 50 ms only (attack weighs as the ear gives it). See change: add-procedural-gm-bank 6.2c.
- `d_env`: mean |dB| of RMS envelopes (window ≥ 2 periods and ≥ 10 ms, every 2 ms, floor −80 dB). Looped part compared after 100 ms smoothing + wobble-depth (fine-envelope std) difference, so shimmer of right depth is not punished.
- `d_loop`: exact loop DFT magnitude |dB| (`LOOP_RANGE` 60; 0 unlooped).
- `d_per` = `periodicity` difference (weight `W_PER` 1) → flag `tone_off` (> 0.25). See change: add-procedural-gm-bank D4l, 7.2.
- `d_mod` (weight `W_MOD` 1): strongest partial flutter + pitch movement, only tonal (share > 0.6) and ≥ 100 ms; flag `mod_off` (> 0.5). See change: add-procedural-gm-bank D4m.
- `score = exp(−(d_spec/12 + d_onset/12 + d_env/6 + d_loop/12 + d_per/1 + d_mod/1))`, 1 for identical.
- Flags: `silent`, `clipped` (orig not clipped), `nan`, `env_off` (10 ms span > 6 dB off where orig > −40 dB), `pitch_off` (autocorr, 5 cents), `loop_click` (wrap curvature > 4× inside), `buzz` (`buzz`/`buzz_band` per-period peakiness, copy > 1.4× orig; See change: add-procedural-gm-bank 6.2f), `tone_off`, `mod_off`.
- `Score::describe` (not in `score`/`fit::score_of`, only `evaluate`): adds `desc` + flags `lines` (`LINES_DB` 6), `transient_off` (`PERC_DIFF` 0.2), `peaks_off` (`PEAKS_RATIO` 1.6) via `desc.rs`. `Score.heard` = 5 hearing values. `Score::json`. See change: add-procedural-gm-bank D4n.
- Helpers: `mel_spec`, `rms_env`, `rms_env_win`, `loop_distance`, `pitch`, `min_window`, `r4`.
- Tests: identical scores 1, silence < 0.1, aligned upper partials buzz, detune → `pitch_off`, independent noises equal, loop shimmer by depth, A-weight reference points, missing click penalised, wrap step → `loop_click`.
