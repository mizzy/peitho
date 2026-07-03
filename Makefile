# 動作確認用: 各サンプルデッキをcargo runでpresentする。
# 発表者画面を窓で開くなら <target>-windowed。その他の追加フラグは PRESENT_FLAGS で渡す。

PRESENT_FLAGS ?=
PRESENT = cargo run -q -p peitho -- present
PEITHO = cargo run -q -p peitho --
DEMO_OUT = .demo-site
DEMO_DECKS = minimal lightning-talk code-walkthrough keynote
WRANGLER ?= npx -y wrangler

.PHONY: help minimal lightning-talk code-walkthrough keynote shell \
	minimal-windowed lightning-talk-windowed code-walkthrough-windowed keynote-windowed \
	demo-site deploy-demo

help:
	@echo "サンプルの動作確認ターゲット:"
	@echo "  make minimal           最小デッキ（内蔵デフォルトテーマ）"
	@echo "  make lightning-talk    日本語LT（ダーク+大型タイポ）"
	@echo "  make code-walkthrough  typestate解説（ターミナル風2カラム）"
	@echo "  make keynote           キーノート（セリフ体中央寄せ）"
	@echo ""
	@echo "発表者画面を窓で開く: make keynote-windowed など <target>-windowed"
	@echo "その他の追加フラグ:   make keynote PRESENT_FLAGS=\"--port 8000\""
	@echo ""
	@echo "デモサイト:"
	@echo "  make demo-site    examplesを$(DEMO_OUT)/に組み立てて検査"
	@echo "  make deploy-demo  Cloudflare Pagesへデプロイ（要wrangler認証）"

minimal-windowed: PRESENT_FLAGS += --presenter-windowed
minimal-windowed: minimal

lightning-talk-windowed: PRESENT_FLAGS += --presenter-windowed
lightning-talk-windowed: lightning-talk

code-walkthrough-windowed: PRESENT_FLAGS += --presenter-windowed
code-walkthrough-windowed: code-walkthrough

keynote-windowed: PRESENT_FLAGS += --presenter-windowed
keynote-windowed: keynote

shell:
	cd packages/peitho-present && npm run build

minimal: shell
	$(PRESENT) examples/deck.md $(PRESENT_FLAGS)

lightning-talk: shell
	$(PRESENT) examples/lightning-talk/deck.md $(PRESENT_FLAGS)

code-walkthrough: shell
	$(PRESENT) examples/code-walkthrough/deck.md $(PRESENT_FLAGS)

keynote: shell
	$(PRESENT) examples/keynote/deck.md $(PRESENT_FLAGS)

demo-site:
	rm -rf $(DEMO_OUT)
	mkdir -p $(DEMO_OUT)
	$(PEITHO) build examples/deck.md --out $(DEMO_OUT)/minimal
	$(PEITHO) build examples/lightning-talk/deck.md --out $(DEMO_OUT)/lightning-talk
	$(PEITHO) build examples/code-walkthrough/deck.md --out $(DEMO_OUT)/code-walkthrough
	$(PEITHO) build examples/keynote/deck.md --out $(DEMO_OUT)/keynote
	for d in $(DEMO_DECKS); do \
		$(PEITHO) publish --dist $(DEMO_OUT)/$$d -- true || exit 1; \
	done
	cp demo/index.html $(DEMO_OUT)/index.html

deploy-demo: demo-site
	$(WRANGLER) pages deploy $(DEMO_OUT) --project-name peitho --branch main
