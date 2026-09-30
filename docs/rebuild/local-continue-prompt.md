# ローカルで続けるときのプロンプト

Claude Desktop などのローカルのセッション（Obsidian と PC に届く環境）で、次をそのまま渡す。

```
Photo Curator の再構成の続きをやってほしい。

## 最初に
1. メモリ MEMORY.md を読む。`pwd`・`git worktree list`・`git branch --show-current` で場所を確かめる。既存のワークツリー・ブランチは触らない
2. `git fetch origin`。`origin/feat/mb-next` の `docs/rebuild/` を読む（README.md → next-tasks.md（とくに「ブランチの運用」と表の「状態」）→ 10-再構成プラン.md の §0・§6・§7）
3. まだなら、`docs/rebuild/obsidian-transfer-prompt.md` の手順で Obsidian へ転記する（07章の追記・10章の新規・各章の直し。`next-tasks.md` も 10章の末尾に「次の作業」として写す）

## 作業
- `docs/rebuild/next-tasks.md` の表で「状態」が「未」の U を、上から順（おすすめの順）に進める
- 1 件 = 1 ブランチ（`origin/feat/mb-next` の先端から `fix/mb-uNN-<名前>`）= 1 PR（`feat/mb-next` 向け）
- ワークツリーは `..\photo-curator-mb-work` に作る（無ければ `git worktree add ..\photo-curator-mb-work origin/feat/mb-next --detach`。そこで `git switch -c fix/mb-uNN-… origin/feat/mb-next`）
- 実装は Agent ツール（model: "sonnet"）に 1 件ずつ任せ、返ってきたら自分で差分とテストを確かめてから push・PR を出す
- 確かめたら `feat/mb-next` に `git merge --no-ff` で取り込んで push し、next-tasks.md の「状態」を「済（#PR）」にし、まとめの PR（`feat/mb-next` → `feat/main-base`）の概要の一覧を更新する（**`feat/main-base`・main へは取り込まない。マージはユーザー**）
- サブエージェントへのプロンプトには次を必ず入れる:
  - 作業場所
  - `next-tasks.md` の担当の節
  - 「共通の決まり」
  - 「プランと現実が食い違う・判断が要るときは、続けずに理由を報告して終わる」
  - 「push・PR・merge は禁止」
  - 完了時に `docs/rebuild/10-再構成プラン.md` §7 に 1 行足す
- **U7（並べ方）は実装の前に、プラン案と「ユーザーに確かめること」を私に聞く**
- **U1 は core を変えるので、Android の `.so` の作り直しの手順を先に確かめ、分からなければ私に聞く**
- 既知の落とし穴:
  - `pnpm tauri:build`（`nuxt generate`）は `.nuxt` を消し、動いている開発サーバーを巻き込む
  - Vite の監視が保存を取りこぼすことがある（touch か再起動）
  - ファイルは CRLF で、複数行の置換スクリプトは一致しないので Edit ツールで行う
  - ブラウザペインは前面でないと IntersectionObserver が動かない
  - IndexedDB の版は下げられない
  - ポート 3000 の `pnpm tauri:dev` を止めない
- push・PR と、`feat/mb-next` への取り込みまではしてよい。それ以外の merge・force・rebase・ブランチ削除はしない
```
