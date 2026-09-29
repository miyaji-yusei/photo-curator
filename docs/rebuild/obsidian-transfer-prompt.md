# Obsidian へ転記させるプロンプト（ローカルのセッション用）

ローカル（Claude Desktop などの Obsidian に届く環境）で、次をそのまま渡す。

```
Photo Curator の再構成の記録を、リポジトリから Obsidian へ転記してほしい。

## 場所
- リポジトリ: C:\Users\miyaj\Documents\tools\photo-curator
  `git fetch origin` のあと、いちばん新しい `feat/mb-*` のブランチ（無ければ `feat/main-base`）の
  `docs/rebuild/` を読む（`git show origin/<ブランチ>:docs/rebuild/<ファイル>` でよい。作業ツリーは変えない）
- Obsidian: 「プロジェクト/Photo Curator/Photo Curator 設計書/」（mcp__obsidian__* で読み書き）

## やること
1. 最初にメモリ MEMORY.md を読む。`git worktree list`・`git branch --show-current` で場所を確かめる。既存のワークツリー・ブランチは触らない
2. `docs/rebuild/07-取り込み方針.md` を、07章「07 実装状況と残タスク」の**末尾**に追記する
   - 見出しは「## 取り込み方針（main を土台に）」
   - 既に同じ見出しがあれば、差分だけを足す（重複させない）
   - 07章の frontmatter の `updated` を今日にする
3. `docs/rebuild/10-再構成プラン.md` を、新しいノート「10 再構成プラン（main を土台に）」として作る
   - frontmatter は他の章（title・created・updated・status・tags）に合わせる
   - 既にあれば、§7「実施結果」と変わった節だけを更新する
4. 07章に、10章への 1 行のリンク（`→ [[10 再構成プラン（main を土台に）]]`）を足す（無ければ）
5. 「Photo Curator 設計書」の目次の表に 10章の行を足す（無ければ）
6. 00〜06章は変えない。10章 §8 に「変わること」が書いてあるが、それは段 T16 で行う
7. 終わったら、転記したノートと節の一覧を短く報告する

## してはいけないこと
- リポジトリのファイルを変える・commit・push する
- Obsidian の他のノートを書き換える
```
