# Step 0: 保全記録（2026-07-23 00:13）

## DBバックアップ

`VACUUM INTO` でWALの内容を取り込んだ整合性のある単一ファイルバックアップを作成した。
読み取り専用接続を使用したため、元DBは無変更。

- 元: `C:\Users\miyaj\AppData\Roaming\app.photocurator.desktop\photo-curator.sqlite3`
- 先: `C:\Users\miyaj\AppData\Roaming\app.photocurator.desktop\photo-curator.backup-20260723.sqlite3`（188,416 bytes）

### 復元手順

1. Photo Curator アプリを終了する
2. `photo-curator.sqlite3` / `photo-curator.sqlite3-wal` / `photo-curator.sqlite3-shm` を別名で退避
3. `photo-curator.backup-20260723.sqlite3` を `photo-curator.sqlite3` にコピー
4. アプリを起動（`-wal`/`-shm` は自動再生成される）

## バックアップ時点のDB内容

| 項目 | 値 |
|---|---|
| プロジェクト | `20260630_新宿エール` 1件 |
| フォルダ | `C:\Users\miyaj\Pictures\20260630_新宿エール`（271枚 / 755MB） |
| photos 行数 | 271 |
| `captured_at` が非NULL | 271 / 271 |
| `d_hash` が非NULL | **0 / 271** |
| `fingerprint_mtime` / `fingerprint_size` | **全件 NULL** |
| `is_missing` | 0件 |

`d_hash` が0件なのは、hash処理が単一トランザクションで完了時にしかcommitされず、
キャンセルのたびに全rollbackされていたため（Step 1で修正）。

## Git状態（変更前・作業開始時点）

HEAD: `3c16a69 chore: initialize Tauri and Nuxt desktop app`
ブランチ: `main...origin/main`

**実装本体は全て未コミットのstaged変更**として存在する。unstaged差分はなし（worktree == index）。

```text
M  app.vue
M  assets/main.css
A  composables/useDesktop.ts
M  package.json
M  plugins/vuetify.ts
M  pnpm-lock.yaml
M  src-tauri/Cargo.lock
M  src-tauri/Cargo.toml
M  src-tauri/capabilities/default.json
A  src-tauri/icons/128x128.png
A  src-tauri/icons/icon.ico
M  src-tauri/src/lib.rs
M  src-tauri/tauri.conf.json
A  types/photo.ts
A  utils/tournament.ts
```

`git diff --cached --stat`: 15 files changed, 1933 insertions(+), 55 deletions(-)

### 注意

引き継ぎ書には `MM` / `AM`（staged + unstaged の混在）と記録されていたが、
現時点では解消しており全てstagedのみ。

**これ以降の変更は全てunstagedとして積み上がる。** git操作（add/commit/push/reset）は
ユーザーの明示指示があるまで一切行わない。
