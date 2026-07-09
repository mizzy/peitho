# 動作確認用: 各サンプルデッキをcargo runでpresentする。
# 発表者画面を窓で開くなら <target>-windowed。その他の追加フラグは PRESENT_FLAGS で渡す。

PRESENT_FLAGS ?=
PRESENT = cargo run -q -p peitho -- present
PEITHO = cargo run -q -p peitho --
DEMO_OUT = .demo-site
DEMO_DECKS = minimal lightning-talk code-walkthrough keynote feature-tour two-column image-showcase aspect-ratio-4-3
DOCS_SOURCE_DIR = site/static/deck-sources
WRANGLER ?= npx -y wrangler

.PHONY: help minimal lightning-talk code-walkthrough keynote feature-tour shell \
	minimal-windowed lightning-talk-windowed code-walkthrough-windowed keynote-windowed \
	feature-tour-windowed docs-sources demo-site deploy-demo screenshots

help:
	@echo "サンプルの動作確認ターゲット:"
	@echo "  make minimal           最小デッキ（内蔵デフォルトテーマ）"
	@echo "  make lightning-talk    日本語LT（ダーク+大型タイポ）"
	@echo "  make code-walkthrough  typestate解説（ターミナル風2カラム）"
	@echo "  make keynote           キーノート（セリフ体中央寄せ）"
	@echo "  make feature-tour      機能総ざらい（明示layout・listスロット・複数ノート）"
	@echo ""
	@echo "発表者画面を窓で開く: make keynote-windowed など <target>-windowed"
	@echo "その他の追加フラグ:   make keynote PRESENT_FLAGS=\"--port 8000\""
	@echo ""
	@echo "デモサイト:"
	@echo "  make demo-site    examplesを$(DEMO_OUT)/に組み立てて検査"
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

keynote-windowed: PRESENT_FLAGS += --presenter-windowed
keynote-windowed: keynote

feature-tour-windowed: PRESENT_FLAGS += --presenter-windowed
feature-tour-windowed: feature-tour

shell:
	cd packages/peitho-present && [ -d node_modules ] || npm ci
	cd packages/peitho-present && npm run build

minimal: shell
	$(PRESENT) examples/deck.md $(PRESENT_FLAGS)

lightning-talk: shell
	$(PRESENT) examples/lightning-talk/deck.md $(PRESENT_FLAGS)

code-walkthrough: shell
	$(PRESENT) examples/code-walkthrough/deck.md $(PRESENT_FLAGS)

keynote: shell
	$(PRESENT) examples/keynote/deck.md $(PRESENT_FLAGS)

feature-tour: shell
	$(PRESENT) examples/feature-tour/deck.md $(PRESENT_FLAGS)

docs-sources:
	rm -rf $(DOCS_SOURCE_DIR)
	mkdir -p $(DOCS_SOURCE_DIR)/minimal
	cp examples/deck.md $(DOCS_SOURCE_DIR)/minimal/deck.md
	for d in $(filter-out minimal,$(DEMO_DECKS)); do \
		mkdir -p $(DOCS_SOURCE_DIR)/$$d; \
		cp examples/$$d/deck.md $(DOCS_SOURCE_DIR)/$$d/deck.md || exit 1; \
	done

demo-site:
	rm -rf $(DEMO_OUT)
	mkdir -p $(DEMO_OUT)
	$(PEITHO) build examples/deck.md --out $(DEMO_OUT)/minimal
	$(PEITHO) build examples/lightning-talk/deck.md --out $(DEMO_OUT)/lightning-talk
	$(PEITHO) build examples/code-walkthrough/deck.md --out $(DEMO_OUT)/code-walkthrough
	$(PEITHO) build examples/keynote/deck.md --out $(DEMO_OUT)/keynote
	$(PEITHO) build examples/feature-tour/deck.md --out $(DEMO_OUT)/feature-tour
	$(PEITHO) build examples/two-column/deck.md --out $(DEMO_OUT)/two-column
	$(PEITHO) build examples/image-showcase/deck.md --out $(DEMO_OUT)/image-showcase
	$(PEITHO) build examples/aspect-ratio-4-3/deck.md --out $(DEMO_OUT)/aspect-ratio-4-3
	for d in $(DEMO_DECKS); do \
		$(PEITHO) publish --dist $(DEMO_OUT)/$$d -- true || exit 1; \
	done
	cp demo/index.html $(DEMO_OUT)/index.html

deploy-demo: demo-site
	$(WRANGLER) pages deploy $(DEMO_OUT) --project-name peitho --branch main

screenshots: shell
	./scripts/take-screenshots.sh
