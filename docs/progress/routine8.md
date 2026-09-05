# Routine 8〜13: Android 対応と、その前提となるデスクトップ側の改善

実施日: 2026-07-28〜29（夜間の連続作業）。
利用者の指示により **git 操作（コミット・ブランチ作成）を行った。**
写真原本には触れていない。

親プランは `docs/android-plan.md`。この文書はそこからの実施記録。

---

## 朝いちばんに確認してほしいこと

**実機が USB から外れたため、アプリの動作確認だけが未了。** ビルドは全部通っている。

```powershell
# どちらのブランチでも同じ手順
git checkout feat/android        # SMB で NAS に直接繋ぐ（第一の道）
pnpm android:apk
adb install -r src-tauri/gen/android/app/build/outputs/apk/arm64/debug/app-arm64-debug.apk
```

| ブランチ | 何を試すか |
|---|---|
| `feat/android` | 端末の写真 ＋ **NAS に SMB で直接** |
| `feat/android-saf` | 端末の写真 ＋ **SAF のフォルダ経由**（NAS アプリが対応していれば NAS も） |

見てほしい順:

1. アプリが起動し、写真の権限を聞いてくること
2. 「プロジェクトを作成」で**アルバムの一覧**が出ること
3. アルバムを選んで走査 → 解析 → 選別まで通ること
4. 1 グループが **4 枚**で出ること（10 枚ではない）
5. `feat/android` で「NAS に繋ぐ」→ ホスト/共有/ユーザーを入れて、
   **共有の直下のフォルダが一覧に出る**こと
6. `feat/android-saf` で「フォルダを選ぶ」→ **選択画面に WebAccess A が出るか**
   （出れば SMB を書かずに NAS へ届く）

---

## 1. デスクトップ側（main に入れた）

### Routine 9: 実行環境の能力を明示する

`isDesktop()` の中身は「Tauri ランタイムが在るか」でしかなく、**Android も
Tauri なので true を返していた。** そのままでは 1 グループ 10 枚で出て、
物理キーボードの案内も出る。

「どの実装か」ではなく「何ができるか」を持つ `BackendCapabilities` に置き換えた。
Android の判定は User-Agent。Rust に問い合わせると非同期になり、画面の初期化が濁る。

### Routine 10: 解析の入口をバイト列にする

Android は `content://` / SMB でファイルを持つのでパスが無い。解析が本当に
必要としているのは「バイト列」と「fingerprint」だけなので、`PhotoSource` として
切り出した。

**部分読みを壊さないことが要点。** EXIF 埋め込みサムネイル経路は先頭 26KB で足り、
97.4% がこの経路を通る（実測）。ここを全体読みに一本化すると、NAS 越しに 1 枚
6.7MB を落とすことになり転送量が 260 倍になる。先頭 64KB で試し、APP1 が
収まらなかったときだけ全体を取り直す。

デスクトップの挙動は変えていない。**既存の 53 件が無修正で通ることが根拠。**

### Routine 10b: 表示用サイズ

選別・連写・比較の各画面が原本（1 枚 6.7MB）を**見るたびに**読んでいた。
Android で NAS を読むと 2,000 枚の 1 ラウンドで 13.4GB 流れる。

保存されているサムネイルは**実際には 160×120**（EXIF 埋め込みをそのまま使う）で、
これでは良し悪しを判断できない。「160×120」と「原本」の間が空いているのが
構造的な欠落だった。

- 既定 1024px / 「大きな画像で選別する」で 1536px / 設定で 768〜1920px
- **走査とは分ける。** 表示用は原本を全部読むので、走査に混ぜると解析が桁で
  遅くなる（EXIF サムネイル経路 1.72ms/枚 に対しフルデコード 132ms/枚）
- **下げるときは原本を読み直さない。** 保存済みを縮めるだけで足りる

### Web アプリ（同じく main）

ブラウザ版も取り込み時に表示用を作り、IndexedDB に持つようにした。
**原本はリロードで失われる**ので、ここで作らないと二度と作れない。
これで iPad でリロードしても選別画面が 256px に落ちなくなった。

---

## 2. Android（feat/android）

### Routine 8: 土台

`tauri android init` で生成。**gen/android は追跡する**（Kotlin と
AndroidManifest をこの下に置くため）。

環境で 3 か所つまずいた。README と `scripts/android-apk.mjs` に残した。

| 詰まり | 対処 |
|---|---|
| `tauri android build` が .so を jniLibs へ**シンボリックリンク**で置く。Windows では開発者モードか管理者権限が要る | 同じ手順を**コピー**で行うスクリプト（`pnpm android:apk`） |
| Android Studio 同梱の JBR は **JDK 25**。Gradle 8.14 が対応していない | JDK 21 を別に置いて `JAVA_HOME` をそちらへ |
| `rusqlite` が C を含むので cc-rs が clang を探すが、素の PATH に無い | NDK のツールチェインを cargo に渡す |

identifier は `app.photocurator.desktop` のまま。変えると**デスクトップの
app_data_dir が変わり、既存の星とサムネイルが行方不明になる。**

### Routine 11: 端末の写真

MediaStore のアルバム（bucket）を「フォルダ」の代わりに選ぶ。

- Kotlin の `PhotoAccess` を **JNI で直接**呼ぶ。Tauri のプラグイン機構は
  使わない（必要なのが「一覧」「バイト列」だけで Activity のやりとりが要らない）
- `PhotoRef` が `content://` とローカルパスを見分ける。**DB のスキーマは
  変えない**。既存の行はすべてローカルとして読まれるので移行が要らない
- `readBytes(uri, offset, length)` で**先頭だけ読める**
- SAF のフォルダ選択は使わない。アプリ自身がアルバムを並べられるので、
  システムのピッカーを挟むと Activity Result を Rust まで運ぶ仕掛けが要るだけ
- 権限は起動時に要求し、**結果を待たない**。断られたら一覧が空になるだけ

### Routine 12–13: NAS（SMB）

**プラン最大のリスクだった「smbj が Android でビルドできるか」は通った。**

- smbj（SMB2/3・純 Java・Apache 2.0）を Kotlin から使い、`android_photos` と
  同じ形で JNI 越しに Rust へ
- `PhotoRef::Smb` が `smb://` を見分け、走査・解析・選別はローカルと同じ経路
- **`readBytes(url, offset, length)` があるので部分読みが確実に効く**
- **認証情報は保存しない。** メモリにしか置かず、終了すると消える

gradle で 2 つ詰まった:
- smbj は Java 8 の API を使う。minSdk 24 では脱糖（desugaring）が要る
- bouncycastle と jspecify が同じ META-INF を持ち込んで衝突する

---

## 3. SAF 経由（feat/android-saf）

SMB が実機で繋がらなかったときの代替。`feat/android` から派生。

**NAS のベンダー製アプリが DocumentsProvider として登録されていれば、
SAF のフォルダ選択にその NAS が現れる。** そこを選べば SMB を書かずに済む。

読み出しは `content://` なので **`PhotoRef::Content` がそのまま使える**。
増えたのは「フォルダを選ぶ」「中を数え上げる」の 2 つだけ。

Activity の結果は Rust へ直接返せないので、「開く」→「あとで取りに行く」の
2 段にした。

**部分読みが効くかは提供元しだい。** `openInputStream` を途中で止めれば
そこまでしか転送しない実装が多いが、保証はない。smbj なら offset/length を
プロトコルへ渡せるので確実に効く。**そこが `feat/android` を第一に置く理由。**

---

## 4. 検証

| | 件数 |
|---|---|
| Rust ユニットテスト | **60**（開始時 53） |
| Vitest | **200**（開始時 191） |
| `pnpm typecheck` / `pnpm build` | 通る |
| Android APK（両ブランチ） | 組み上がる |

**ミューテーションで空振りを確認した箇所:**

| 入れた変更 | 落ちたテスト |
|---|---|
| 先頭だけ読まず、いきなり全体を読む | `the_exif_thumbnail_path_never_asks_for_the_whole_file` |
| 保存済みの表示用画像を使い回さない | `shrinking_the_display_size_reuses_the_saved_image` |

新しく足した守り:

- `a_photo_reference_tells_local_paths_from_content_uris`
  … **既存の行を壊さないこと**。判別を間違えると全部が「読めない写真」になる
- `a_content_uri_is_simply_unreadable_off_android`
  … Android 以外で content:// が来ても落ちないこと
- `the_display_edge_is_clamped_to_the_offered_choices`
  … 壊れた設定値で生成する画像が暴れないこと
- `the_display_image_never_upscales_the_original`

---

## 5. 環境に加えた変更（システム側）

利用者の許可を得て入れた。

| | |
|---|---|
| NDK 27.2.12479018 | `sdkmanager` で導入 |
| Rust ターゲット | `aarch64-linux-android` ほか 4 つ |
| JDK 21（Temurin） | `%LOCALAPPDATA%\Programs\jdk21` に展開。管理者権限は不要 |
| 環境変数（User） | `JAVA_HOME` → JDK 21、`ANDROID_HOME`、`NDK_HOME` |

**`JAVA_HOME` を Android Studio の JBR から JDK 21 に向け直した。**
Android Studio 自体は自分の JBR を使うので影響しないが、他のツールが
`JAVA_HOME` を見ている場合は 21 になる。

---

## 6. 未了・注意

- **実機での動作確認が全部未了。** 端末が USB から外れたため。ビルドと
  コードの噛み合いまでは確認済み
- **SMB の実機接続は未検証。** LinkStation の SMB バージョンと認証方式を
  管理画面で確認する必要がある。SMB1 のみなら smbj では繋がらない
- APK が 321MB ある。debug でシンボルを残しているため。
  `pnpm android:apk --release` なら大幅に小さくなる（未検証）
- サイドカー（PC と NAS 経由で結果を共有する仕組み・親プランの Routine 14）と
  厳選フォルダへの書き出し（同 15）は**未着手**
