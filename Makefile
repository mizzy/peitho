# 動作確認用: 各サンプルデッキをcargo runでpresentする。
# 発表者画面を窓で開くなら <target>-windowed。その他の追加フラグは PRESENT_FLAGS で渡す。

PRESENT_FLAGS ?=
PRESENT = cargo run -q -p peitho -- present

.PHONY: help minimal lightning-talk code-walkthrough keynote shell \
	minimal-windowed lightning-talk-windowed code-walkthrough-windowed keynote-windowed

help:
	@echo "サンプルの動作確認ターゲット:"
	@echo "  make minimal           最小デッキ（内蔵デフォルトテーマ）"
	@echo "  make lightning-talk    日本語LT（ダーク+大型タイポ）"
	@echo "  make code-walkthrough  typestate解説（ターミナル風2カラム）"
	@echo "  make keynote           キーノート（セリフ体中央寄せ）"
	@echo ""
	@echo "発表者画面を窓で開く: make keynote-windowed など <target>-windowed"
	@echo "その他の追加フラグ:   make keynote PRESENT_FLAGS=\"--port 8000\""

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
	$(PRESENT) examples/lightning-talk/deck.md \
		--layout examples/lightning-talk/layout.html \
		--base-css examples/lightning-talk/base.css \
		--overrides-css examples/lightning-talk/overrides.css \
		$(PRESENT_FLAGS)

code-walkthrough: shell
	$(PRESENT) examples/code-walkthrough/deck.md \
		--layout examples/code-walkthrough/layout.html \
		--base-css examples/code-walkthrough/base.css \
		--overrides-css examples/code-walkthrough/overrides.css \
		$(PRESENT_FLAGS)

keynote: shell
	$(PRESENT) examples/keynote/deck.md \
		--layout examples/keynote/layout.html \
		--base-css examples/keynote/base.css \
		--overrides-css examples/keynote/overrides.css \
		$(PRESENT_FLAGS)
