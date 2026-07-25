import { defineConfig } from 'vitest/config'

// @types/node を足さずに済ませたいので node:url は使わない。
// Windows では pathname が "/C:/..." になるため先頭のスラッシュを落とす。
const root = new URL('.', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')

export default defineConfig({
  // Nuxt が解決している `~` / `@` を vitest 単体でも使えるようにする。
  // これが無いと `~/utils/...` を import したテストが収集時に落ちる。
  resolve: {
    alias: { '~': root, '@': root, '~~': root, '@@': root }
  },
  test: {
    // Claude Code の worktree はリポジトリ内の .claude/worktrees/ 配下に
    // 作業ツリーごと複製される。除外しないと同じテストが二重に収集され、
    // .nuxt/ を持たない複製側の tsconfig 解決が失敗して常に赤くなる。
    exclude: [
      '**/node_modules/**',
      '**/dist/**',
      '.claude/**',
      '.nuxt/**',
      '.output/**',
      'src-tauri/**'
    ]
  }
})
