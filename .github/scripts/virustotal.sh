#!/usr/bin/env bash
# Uploads release packages to VirusTotal, waits for the scans and writes a Markdown summary.
# Usage: VT_API_KEY=… virustotal.sh <file>… > virustotal.md
# Report only: detections never fail the release (machine-learning engines produce occasional false positives).
# Free public API: 4 requests/minute, so uploads and polls are spaced out.
set -euo pipefail
api=https://www.virustotal.com/api/v3
[ -n "${VT_API_KEY:-}" ] || { echo "VT_API_KEY is not set: skipping the VirusTotal scan" >&2; exit 0; }

echo "### VirusTotal"
echo
echo "| File | Result | Report |"
echo "|---|---|---|"
for f in "$@"; do
  name=$(basename "$f")
  sha=$(sha256sum "$f" | cut -c1-64)
  report="https://www.virustotal.com/gui/file/$sha"
  # files over 32 MB need a special upload URL
  url="$api/files"
  if [ "$(stat -c %s "$f")" -gt 33554432 ]; then
    url=$(curl -fsS -H "x-apikey: $VT_API_KEY" "$api/files/upload_url" | jq -r .data)
  fi
  analysis=$(curl -fsS -H "x-apikey: $VT_API_KEY" -F "file=@$f" "$url" | jq -r .data.id)
  sleep 20
  stats=""
  for _ in $(seq 1 40); do
    r=$(curl -fsS -H "x-apikey: $VT_API_KEY" "$api/analyses/$analysis")
    if [ "$(echo "$r" | jq -r .data.attributes.status)" = completed ]; then
      stats=$(echo "$r" | jq -r '.data.attributes.stats')
      break
    fi
    sleep 30
  done
  if [ -z "$stats" ]; then
    echo "| \`$name\` | scan still running (check the report later) | [report]($report) |"
    continue
  fi
  mal=$(echo "$stats" | jq '.malicious'); sus=$(echo "$stats" | jq '.suspicious')
  total=$(echo "$stats" | jq '[.malicious, .suspicious, .undetected, .harmless] | add')
  if [ "$mal" -eq 0 ] && [ "$sus" -eq 0 ]; then
    result="✅ clean: 0 of $total engines flagged it"
  else
    result="⚠️ $mal malicious, $sus suspicious of $total engines"
  fi
  echo "| \`$name\` | $result | [report]($report) |"
  sleep 20
done
echo
echo "Scanned right after the build; the report link always shows VirusTotal's latest result."
