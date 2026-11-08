#!/usr/bin/env bash
# Publish the site's audio to the public bucket (default gs://gm-procedural, CORS *).
#   site/songs/<id>/{mix,ch<N>}.mp3, site/waves/NNN.mp3   gm.dls renders (tools/spessa/render.ts) — the only gm.dls audio
#   site/midi/<id>.mid                                     the songs, played live by the site
#   site/banks/gmplus.sf2, gen-gm.dls, gen-plus.dls        live banks: gen-gm is loaded by default (the wasm's output,
#                                                          sha1-checked by the page); gen-plus is a fallback (the page builds it)
#   site/gmgen.wasm                                        the bank generator the page runs (all binaries live here, not on Pages)
#   gm/pages/<name>/, index.html                           hand-made fitting-round pages (pages/) + their clips (build/gm/<name>/)
# Never uploads gm.dls itself, logs or temporary files. Usage: tools/publish.sh [--dry-run]
set -euo pipefail
cd "$(dirname "$0")/.."
B="${GM_BUCKET:-gs://gm-procedural}"
DRY=""; [[ "${1:-}" == "--dry-run" ]] && DRY="--dry-run"   # string, not array: macOS bash 3.2 + set -u
NEVER='.*\.(dls|DLS|sf2|log|txt|json|tmp\.wav)$'

A=build/site-audio
for d in songs waves midi; do
  if [[ -d "$A/$d" ]]; then gcloud storage rsync -r $DRY --exclude="$NEVER" "$A/$d" "$B/site/$d"; fi
done
if [[ -z "$DRY" ]]; then
  gcloud storage cp -n "${GMPLUS_SF2:?set GMPLUS_SF2}" "$B/site/banks/gmplus.sf2"
  # gen-gm as written by bun tools/site/build.ts (gmgen.wasm output, its sha1 is bundled in live.js)
  gcloud storage cp "$A/banks/gen-gm.dls" "$B/site/banks/gen-gm.dls"
  gcloud storage cp build/gm-plus/gen.dls "$B/site/banks/gen-plus.dls"
  # the bank generator the site runs in the browser (bun tools/site/build.ts puts it here); fetched as ?v=<hash>
  gcloud storage cp --content-type=application/wasm "$A/gmgen.wasm" "$B/site/gmgen.wasm"
fi

# Hand-made fitting-round pages: HTML from pages/<name>/, audio + sheets from build/gm/<name>/.
for p in pages/*/; do
  n=$(basename "$p")
  if [[ -d "build/gm/$n" ]]; then
    gcloud storage rsync -r $DRY --exclude='.*\.(dls|sf2|log|txt|html)$' "build/gm/$n" "$B/gm/pages/$n"
  fi
  if [[ -z "$DRY" ]]; then gcloud storage cp --cache-control=no-cache "$p/index.html" "$B/gm/pages/$n/index.html"; fi
done
if [[ -z "$DRY" ]]; then gcloud storage cp --cache-control=no-cache pages/index.html "$B/index.html"; fi
echo "https://gm-procedural.exceed.hu/  (audio: https://storage.googleapis.com/${B#gs://}/site/)"
