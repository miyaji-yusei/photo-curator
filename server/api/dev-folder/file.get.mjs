// 開発用フォルダ配信の続き。1 ファイルの中身を返す（EXIF・サムネ生成・拡大用）。
// 本番には入らない（`process.dev` で切る）。
import { readFile, stat } from 'node:fs/promises'
import { join, extname } from 'node:path'
import { defineEventHandler, getQuery, createError, setHeader, getHeader, send } from 'h3'

const CONTENT_TYPES = {
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.png': 'image/png',
  '.webp': 'image/webp',
  '.heic': 'image/heic',
  '.heif': 'image/heif',
  '.mp4': 'video/mp4',
  '.mov': 'video/quicktime'
}

export default defineEventHandler(async (event) => {
  if (!process.dev) {
    throw createError({ statusCode: 404, statusMessage: 'Not Found' })
  }
  const query = getQuery(event)
  const root = typeof query.root === 'string' ? query.root : ''
  const sub = typeof query.path === 'string' ? query.path : ''
  if (!root || !sub) throw createError({ statusCode: 400, statusMessage: 'root/path is required' })

  const target = join(root, sub)
  const info = await stat(target)
  const ext = extname(target).toLowerCase()
  setHeader(event, 'Content-Type', CONTENT_TYPES[ext] ?? 'application/octet-stream')
  setHeader(event, 'Content-Length', String(info.size))
  setHeader(event, 'Cache-Control', 'no-store')

  // 先頭 64KB だけ欲しい場合（EXIF 用）。Range を簡易に扱う。
  const range = getHeader(event, 'range')
  if (range) {
    const match = /bytes=(\d+)-(\d+)?/.exec(range)
    if (match) {
      const start = Number(match[1])
      const end = match[2] ? Number(match[2]) : info.size - 1
      const buffer = await readFile(target)
      const slice = buffer.subarray(start, end + 1)
      setHeader(event, 'Content-Range', `bytes ${start}-${end}/${info.size}`)
      setHeader(event, 'Content-Length', String(slice.length))
      event.node.res.statusCode = 206
      return send(event, slice)
    }
  }

  const buffer = await readFile(target)
  return send(event, buffer)
})
