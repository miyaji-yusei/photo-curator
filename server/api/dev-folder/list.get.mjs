// 開発用。`pnpm dev` のときだけ、指定フォルダを読むだけで配信する
// （設計 07 章 段2-3）。OS のダイアログ無しでブラウザペインから画面を
// 確かめるための入口。**本番のビルドには入らない**（`process.dev` で切る）。
//
// node:fs を使うため .mjs にしている（@types/node を足さない方針。
// tests/core-wasm-fixtures.test.mjs と同じ理由）。
import { readdir, stat } from 'node:fs/promises'
import { join } from 'node:path'
import { defineEventHandler, getQuery, createError } from 'h3'

export default defineEventHandler(async (event) => {
  if (!process.dev) {
    throw createError({ statusCode: 404, statusMessage: 'Not Found' })
  }
  const query = getQuery(event)
  const root = typeof query.root === 'string' ? query.root : ''
  const sub = typeof query.path === 'string' ? query.path : ''
  if (!root) throw createError({ statusCode: 400, statusMessage: 'root is required' })

  const target = sub ? join(root, sub) : root
  const entries = await readdir(target, { withFileTypes: true })
  const result = []
  for (const entry of entries) {
    // 隠しフォルダ・.photo-curator は出さない。
    if (entry.name.startsWith('.')) continue
    const full = join(target, entry.name)
    try {
      const info = await stat(full)
      result.push({
        name: entry.name,
        isDirectory: entry.isDirectory(),
        size: info.size,
        mtimeMs: info.mtimeMs
      })
    } catch {
      // 読めないものは黙って外す。
    }
  }
  return result
})
