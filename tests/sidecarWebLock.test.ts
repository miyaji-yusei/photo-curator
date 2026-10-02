// @vitest-environment node
/**
 * U52 D4・D5: Web（File System Access の HandleFolderIO）のサイドカーの退避とロック。
 * 退避の名前・数とロックの古さの判断は Android の U44（と PC の Rust）と同じ。
 */
import { describe, expect, it } from 'vitest'
import { HandleFolderIO } from '~/composables/backends/web/sourceIO'

interface MemFile { text: string, lastModified: number }

/** `.photo-curator/` だけを持つ、メモリ上の偽のフォルダ。 */
function memoryRoot() {
  const files = new Map<string, MemFile>()
  const notFound = () => new DOMException('no', 'NotFoundError')
  const fileHandle = (name: string) => ({
    kind: 'file' as const,
    getFile: async () => {
      const file = files.get(name)
      if (!file) throw notFound()
      return { text: async () => file.text, lastModified: file.lastModified }
    },
    createWritable: async () => {
      let buffer = ''
      return {
        write: async (data: string) => { buffer += data },
        close: async () => { files.set(name, { text: buffer, lastModified: Date.now() }) },
        abort: async () => undefined
      }
    }
  })
  const dir = {
    kind: 'directory' as const,
    async getFileHandle(name: string, options?: { create?: boolean }) {
      if (!files.has(name)) {
        if (!options?.create) throw notFound()
        files.set(name, { text: '', lastModified: Date.now() })
      }
      return fileHandle(name)
    },
    async removeEntry(name: string) {
      if (!files.delete(name)) throw notFound()
    },
    async *entries() {
      for (const name of [...files.keys()]) yield [name, fileHandle(name)] as const
    }
  }
  const root = {
    kind: 'directory' as const,
    async getDirectoryHandle(name: string) {
      if (name !== '.photo-curator') throw notFound()
      return dir
    }
  }
  return { root, files }
}

const io = (root: unknown) => new HandleFolderIO(root as never)

describe('U52 D5: Web のロック（Android と同じ古さの判断）', () => {
  it('中身の at が新しければ、更新時刻が古くても壊さない', async () => {
    const { root, files } = memoryRoot()
    files.set('catalog.json', { text: 'v1', lastModified: 1 })
    files.set('catalog.lock', { text: JSON.stringify({ holder: 'android', at: Date.now() }), lastModified: Date.now() - 120_000 })
    expect(await io(root).writeSidecarChecked('v2', 'v1')).toBe('locked')
    expect(files.get('catalog.json')!.text).toBe('v1')
    expect(files.has('catalog.lock')).toBe(true)
  })

  it('at と更新時刻の両方が古ければ壊して書く。at が古くても更新時刻が新しければ壊さない', async () => {
    const { root, files } = memoryRoot()
    files.set('catalog.json', { text: 'v1', lastModified: 1 })
    const old = JSON.stringify({ holder: 'android', at: Date.now() - 120_000 })
    files.set('catalog.lock', { text: old, lastModified: Date.now() })
    expect(await io(root).writeSidecarChecked('v2', 'v1')).toBe('locked')
    files.set('catalog.lock', { text: old, lastModified: Date.now() - 120_000 })
    expect(await io(root).writeSidecarChecked('v2', 'v1')).toBe('written')
    expect(files.get('catalog.json')!.text).toBe('v2')
    expect(files.has('catalog.lock')).toBe(false)
  })

  it('中身が空なら、更新時刻が古いときだけ壊す', async () => {
    const { root, files } = memoryRoot()
    files.set('catalog.json', { text: 'v1', lastModified: 1 })
    files.set('catalog.lock', { text: '', lastModified: Date.now() })
    expect(await io(root).writeSidecarChecked('v2', 'v1')).toBe('locked')
    files.set('catalog.lock', { text: '', lastModified: Date.now() - 120_000 })
    expect(await io(root).writeSidecarChecked('v2', 'v1')).toBe('written')
  })

  it('取ったロックは Android・PC と同じ {"holder","at"} の形', async () => {
    const { root, files } = memoryRoot()
    let seen = ''
    const original = root.getDirectoryHandle.bind(root)
    root.getDirectoryHandle = async (name: string) => {
      const dir = await original(name)
      const getFileHandle = dir.getFileHandle.bind(dir)
      dir.getFileHandle = async (file: string, options?: { create?: boolean }) => {
        if (file === 'catalog.json' && files.has('catalog.lock')) seen = files.get('catalog.lock')!.text
        return getFileHandle(file, options)
      }
      return dir
    }
    expect(await io(root).writeSidecarChecked('v1', null)).toBe('written')
    const lock = JSON.parse(seen)
    expect(typeof lock.holder).toBe('string')
    expect(typeof lock.at).toBe('number')
    expect(files.has('catalog.lock')).toBe(false)
  })
})

describe('U52 D4: Web の NAS の退避', () => {
  it('確かめたあとで置き換える版を時刻つきの名前に退避し、見た版と違えば退避もしない', async () => {
    const { root, files } = memoryRoot()
    files.set('catalog.json', { text: 'android-v1', lastModified: 1 })
    expect(await io(root).writeSidecarChecked('pc', 'old', 'zzzzzzzz-111')).toBe('changed')
    expect([...files.keys()]).toEqual(['catalog.json'])
    expect(await io(root).writeSidecarChecked('pc', 'android-v1', 'zzzzzzzz-111')).toBe('written')
    const asides = [...files.keys()].filter(name => /^catalog\.zzzzzzzz-111\.\d{14}\.json$/.test(name))
    expect(asides).toHaveLength(1)
    expect(files.get(asides[0]!)!.text).toBe('android-v1')
  })

  it('上書きせず（同じ秒は -2）、同じ印の時刻つきは新しい 5 つだけ残す', async () => {
    const { root, files } = memoryRoot()
    files.set('catalog.abc.json', { text: 'old', lastModified: 1 })
    const target = io(root)
    const names: string[] = []
    for (let index = 0; index < 7; index++) names.push(await target.asideSidecar(`v${index}`, 'abc'))
    expect(new Set(names).size).toBe(7)
    const kept = [...files.keys()].filter(name => name.startsWith('catalog.abc.2'))
    expect(kept).toHaveLength(5)
    expect(files.get('catalog.abc.json')!.text).toBe('old')
    await expect(target.asideSidecar('x', 'a/b')).rejects.toThrow()
  })
})
