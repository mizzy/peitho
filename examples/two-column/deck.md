---
time: 3m
---

# 明示スロットで左右2カラム

::: {slot=left}

慣習マッピングだけでは`left`と`right`を判別できない。

同じ`accepts=blocks`が2つあると、どちらに入れるべきかが決められない。

:::

::: {slot=right}

`::: {slot=name}`で著者が明示指定する。

- Markdownネイティブな拡張(HTMLタグを持ち込まない)
- パーサから型で運ばれる
- silent dropは発生しない

:::

---

# 比較: 慣習 vs 明示

::: {slot=left}

## 慣習マッピング

見出しは`title`、コードは`code`、それ以外はまとめて`body`。単一slotで済むレイアウトなら1行も余計な記述はいらない。

:::

::: {slot=right}

## 明示指定

著者が `::: {slot=name}` で意図をコードとして示す。開閉は3コロン、属性は`{slot=name}`だけ。

複数の`blocks` slotを持つレイアウトで、慣習では判別できない曖昧さを著者側から解消する。

:::

---

# エラーは行番号+ヘルプで返る

::: {slot=left}

## パース段

- 開閉不一致
- 属性の不正
- ネスト(v1未対応)
- 空の`:::`ブロック
- 4コロン以上のフェンス(将来予約)

:::

::: {slot=right}

## マッピング段

レイアウトに存在しないslot名を明示すると、その場でエラーになる。

- 「unknown slot 'middle' in explicit `::: {slot=...}` for layout 'two-column'」
- help: 「use one of: left, right, title」

:::
