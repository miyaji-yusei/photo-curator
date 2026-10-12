// @vitest-environment node
/**
 * W10: フォルダ（handle）の走査は、写真ごとに getFile() を呼ばない（呼ぶたびにブラウザとファイル
 * システムの往復が入る）。読み込み（readFile）の階層の辿りは、同じ階層なら 1 回だけ。
 */
import { beforeAll, describe, expect, it } from 'vitest'
import { HandleFolderIO } from '~/composables/backends/web/sourceIO'
import { scanFolder } from '~/utils/folderScan'
import { initWithBytes } from '~/lib/core'
import { wasmBytes } from './helpers/wasmBytes.mjs'

// 走査の最後の「組の RAW を除く」は core（wasm）が持つ（R10）。
beforeAll(() => initWithBytes(wasmBytes()))

interface Counts { getFile: number, getDirectoryHandle: number }

function fakeDir(files: string[], dirs: Record<string, unknown>, counts: Counts) {
  return {
    kind: 'directory' as const,
    async *entries() {
      for (const name of Object.keys(dirs)) yield [name, dirs[name]] as const
      for (const name of files) {
        yield [name, {
          kind: 'file' as const,
          getFile: async () => { counts.getFile += 1; return new File(['x'], name, { lastModified: 5 }) }
        }] as const
      }
    },
    async getDirectoryHandle(name: string) {
      counts.getDirectoryHandle += 1
      const found = dirs[name]
      if (!found) throw new DOMException('no', 'NotFoundError')
      return found
    },
    async getFileHandle(name: string) {
      return { getFile: async () => { counts.getFile += 1; return new File(['x'], name) } }
    }
  }
}

function tree(perDir: number, counts: Counts) {
  const names = (prefix: string) => Array.from({ length: perDir }, (_, i) => `${prefix}${i}.jpg`)
  const deep = fakeDir(names('deep'), {}, counts)
  const mid = fakeDir(names('mid'), { deep }, counts)
  return fakeDir(names('top'), { a: fakeDir(names('a'), { mid }, counts) }, counts)
}

describe('HandleFolderIO', () => {
  it('走査（list）は getFile を 1 回も呼ばず、写真を全部見つける', async () => {
    const counts = { getFile: 0, getDirectoryHandle: 0 }
    const io = new HandleFolderIO(tree(50, counts) as never)
    const files = await scanFolder(io)
    expect(files).toHaveLength(200)
    expect(counts.getFile).toBe(0)
    console.log(`[bench] 走査 200 枚: getFile ${counts.getFile} 回`)
  })

  it('readFile は同じ階層の getDirectoryHandle を使い回す', async () => {
    const counts = { getFile: 0, getDirectoryHandle: 0 }
    const io = new HandleFolderIO(tree(50, counts) as never)
    for (let i = 0; i < 50; i += 1) await io.readFile('a/mid/deep', `deep${i}.jpg`)
    expect(counts.getDirectoryHandle).toBe(3)
    console.log(`[bench] 深さ 3 の 50 枚の読み込み: getDirectoryHandle ${counts.getDirectoryHandle} 回`)
  })
})
