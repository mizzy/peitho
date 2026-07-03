# 動作確認用: 各サンプルデッキをcargo runでpresentする。
# 追加フラグは PRESENT_FLAGS で渡す。例:
#   make keynote PRESENT_FLAGS="--presenter-windowed"

PRESENT_FLAGS ?=
PRESENT = cargo run -q -p peitho -- present

.PHONY: help minimal lightning-talk code-walkthrough keynote shell

help:
	@echo "サンプルの動作確認ターゲット:"
	@echo "  make minimal           最小デッキ（内蔵デフォルトテーマ）"
	@echo "  make lightning-talk    日本語LT（ダーク+大型タイポ）"
	@echo "  make code-walkthrough  typestate解説（ターミナル風2カラム）"
	@echo "  make keynote           キーノート（セリフ体中央寄せ）"
	@echo ""
	@echo "追加フラグ: make keynote PRESENT_FLAGS=\"--presenter-windowed\""

shell:
	cd packages/peitho-present && npm run build

minimal: shell
	$(PRESENT) examples/deck.md $(PRESENT_FLAGS)

lightning-talk: shell
	$(PRESENT) examples/lightning-talk/deck.md \
		--template examples/lightning-talk/template.html \
		--base-css examples/lightning-talk/base.css \
		--overrides-css examples/lightning-talk/overrides.css \
		$(PRESENT_FLAGS)

code-walkthrough: shell
	$(PRESENT) examples/code-walkthrough/deck.md \
		--template examples/code-walkthrough/template.html \
		--base-css examples/code-walkthrough/base.css \
		--overrides-css examples/code-walkthrough/overrides.css \
		$(PRESENT_FLAGS)

keynote: shell
	$(PRESENT) examples/keynote/deck.md \
		--template examples/keynote/template.html \
		--base-css examples/keynote/base.css \
		--overrides-css examples/keynote/overrides.css \
		$(PRESENT_FLAGS)
