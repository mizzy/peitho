#!/usr/bin/env bash
# READMEに載せるスクリーンショットをdocs/images/に再生成する。
# 実装が変わったら make screenshots で撮り直してコミットする。
#
# 仕組み:
# - examples: peitho buildの出力をローカルHTTPサーバー越しにheadless Chromeで撮る
#   (dist/index.htmlはslides/*.htmlをfetchするのでfile://では真っ黒になる)
# - presenter: peitho presentを--no-openで立ち上げ、--timeoutで強制キャプチャする
#   (presenterは/syncのlong-pollを張りっぱなしにするためロード完了を待つと永久にハングする)
set -euo pipefail

CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD_DIR="$ROOT/.screenshots/build"
IMG_DIR="$ROOT/docs/images"
HTTP_PORT=8765
PRESENT_PORT=8766
WINDOW_SIZE=1600,900

cargo build -q -p peitho
PEITHO="$ROOT/target/debug/peitho"

rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR" "$IMG_DIR"

cleanup() {
  [[ -n "${HTTP_PID:-}" ]] && kill "$HTTP_PID" 2>/dev/null || true
  # presentは/syncへのcloseブロードキャストで正常終了させる（SIGTERMはChromeにクラッシュ扱いされる）
  curl -s -X POST -d '{"close":true}' "http://127.0.0.1:$PRESENT_PORT/sync" --max-time 3 >/dev/null 2>&1 || true
}
trap cleanup EXIT

# --- examples ---
declare -a DECKS=(
  "minimal:examples/minimal/deck.md"
  "lightning-talk:examples/lightning-talk/deck.md"
  "code-walkthrough:examples/code-walkthrough/deck.md"
  "keynote:examples/keynote/deck.md"
  "peitho-tour:examples/peitho-tour/deck.md"
)

for entry in "${DECKS[@]}"; do
  name="${entry%%:*}"
  deck="${entry#*:}"
  "$PEITHO" build "$ROOT/$deck" --out "$BUILD_DIR/$name"
done

python3 -m http.server "$HTTP_PORT" --directory "$BUILD_DIR" >/dev/null 2>&1 &
HTTP_PID=$!
sleep 1

for entry in "${DECKS[@]}"; do
  name="${entry%%:*}"
  "$CHROME" --headless=new --hide-scrollbars --window-size="$WINDOW_SIZE" \
    --screenshot="$IMG_DIR/example-$name.png" \
    --virtual-time-budget=3000 \
    "http://127.0.0.1:$HTTP_PORT/$name/index.html" 2>/dev/null
  echo "wrote docs/images/example-$name.png"
done

kill "$HTTP_PID" 2>/dev/null || true
HTTP_PID=

# --- presenter ---
"$PEITHO" present "$ROOT/examples/lightning-talk/deck.md" \
  --presenter-windowed --port "$PRESENT_PORT" --no-open >/dev/null 2>&1 &
sleep 3

"$CHROME" --headless=new --hide-scrollbars --window-size="$WINDOW_SIZE" \
  --screenshot="$IMG_DIR/presenter-view.png" \
  --timeout=8000 \
  "http://127.0.0.1:$PRESENT_PORT/presenter" 2>/dev/null
echo "wrote docs/images/presenter-view.png"
