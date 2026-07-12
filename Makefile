# 動作確認用: 各サンプルデッキをcargo runでpresentする。
# 発表者画面を窓で開くなら <target>-windowed。その他の追加フラグは PRESENT_FLAGS で渡す。

PRESENT_FLAGS ?=
PRESENT = cargo run -q -p peitho -- present
PEITHO = cargo run -q -p peitho --
ZOLA_VERSION = 0.22.1
ZOLA ?= zola
ZOLA_BASE_URL ?=
DEMO_OUT = .demo-site
DEMO_OUT_ABS := $(abspath $(DEMO_OUT))
DEMO_DECKS = minimal lightning-talk code-walkthrough code-images keynote peitho-tour two-column image-showcase aspect-ratio-4-3
DEMO_SCREENSHOTS_DIR = $(DEMO_OUT)/deck-shots
DOCS_SOURCE_DIR = site/static/deck-sources
WRANGLER ?= npx -y wrangler

.PHONY: help minimal lightning-talk code-walkthrough code-images keynote peitho-tour shell \
	minimal-windowed lightning-talk-windowed code-walkthrough-windowed code-images-windowed keynote-windowed \
	peitho-tour-windowed docs-sources demo-site demo-screenshots og-cards deploy-demo screenshots

help:
	@echo "サンプルの動作確認ターゲット:"
	@echo "  make minimal           最小デッキ（内蔵デフォルトテーマ）"
	@echo "  make lightning-talk    日本語LT（ダーク+大型タイポ）"
	@echo "  make code-walkthrough  typestate解説（ターミナル風2カラム）"
	@echo "  make code-images       code_images図解（Mermaid/GraphvizをSVG化）"
	@echo "  make keynote           キーノート（セリフ体中央寄せ）"
	@echo "  make peitho-tour      Peithoの機能ツアー（4レイアウト・画像スロット・複数ノート）"
	@echo ""
	@echo "発表者画面を窓で開く: make keynote-windowed など <target>-windowed"
	@echo "その他の追加フラグ:   make keynote PRESENT_FLAGS=\"--port 8000\""
	@echo ""
	@echo "デモサイト:"
	@echo "  make demo-site    build the docs site plus every demo deck into $(DEMO_OUT)/"
	@echo "  make deploy-demo  Cloudflare Pagesへデプロイ（要wrangler認証）"
	@echo ""
	@echo "README用スクリーンショット:"
	@echo "  make screenshots  docs/images/をheadless Chromeで再生成"

minimal-windowed: PRESENT_FLAGS += --presenter-windowed
minimal-windowed: minimal

lightning-talk-windowed: PRESENT_FLAGS += --presenter-windowed
lightning-talk-windowed: lightning-talk

code-walkthrough-windowed: PRESENT_FLAGS += --presenter-windowed
code-walkthrough-windowed: code-walkthrough

code-images-windowed: PRESENT_FLAGS += --presenter-windowed
code-images-windowed: code-images

keynote-windowed: PRESENT_FLAGS += --presenter-windowed
keynote-windowed: keynote

peitho-tour-windowed: PRESENT_FLAGS += --presenter-windowed
peitho-tour-windowed: peitho-tour

shell:
	cd packages/peitho-present && [ -d node_modules ] || npm ci
	cd packages/peitho-present && npm run build

minimal: shell
	$(PRESENT) examples/minimal/deck.md $(PRESENT_FLAGS)

lightning-talk: shell
	$(PRESENT) examples/lightning-talk/deck.md $(PRESENT_FLAGS)

code-walkthrough: shell
	$(PRESENT) examples/code-walkthrough/deck.md $(PRESENT_FLAGS)

code-images: shell
	$(PRESENT) examples/code-images/deck.md $(PRESENT_FLAGS)

keynote: shell
	$(PRESENT) examples/keynote/deck.md $(PRESENT_FLAGS)

peitho-tour: shell
	$(PRESENT) examples/peitho-tour/deck.md $(PRESENT_FLAGS)

docs-sources:
	rm -rf $(DOCS_SOURCE_DIR)
	for d in $(DEMO_DECKS); do \
		mkdir -p $(DOCS_SOURCE_DIR)/$$d; \
		cp examples/$$d/deck.md $(DOCS_SOURCE_DIR)/$$d/deck.md || exit 1; \
	done

demo-site: docs-sources
	rm -rf $(DEMO_OUT)
	zola_log=$$(mktemp); \
	(cd site && $(ZOLA) build --output-dir $(DEMO_OUT_ABS) $(if $(ZOLA_BASE_URL),--base-url $(ZOLA_BASE_URL))) >$$zola_log 2>&1; \
	zola_status=$$?; \
	cat $$zola_log; \
	if [ $$zola_status -ne 0 ]; then rm -f $$zola_log; exit $$zola_status; fi; \
	if grep -Fq 'page(s) ignored' $$zola_log; then rm -f $$zola_log; exit 1; fi; \
	rm -f $$zola_log
	for d in $(DEMO_DECKS); do \
		$(PEITHO) build examples/$$d/deck.md --out $(DEMO_OUT)/demo/$$d || exit 1; \
	done
	for d in $(DEMO_DECKS); do \
		$(PEITHO) publish --dist $(DEMO_OUT)/demo/$$d -- true || exit 1; \
	done

demo-screenshots:
	mkdir -p $(DEMO_SCREENSHOTS_DIR)
	node scripts/screenshot-decks.mjs $(DEMO_DECKS)

og-cards:
	node scripts/render-og-cards.mjs

deploy-demo: demo-site
	$(WRANGLER) pages deploy $(DEMO_OUT) --project-name peitho --branch main

screenshots: shell
	./scripts/take-screenshots.sh
