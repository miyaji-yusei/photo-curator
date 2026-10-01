// core（wasm）のバイト列を読む。node:fs を使うので .mjs にしてある
// （`core-wasm-fixtures.test.mjs` と同じ理由。@types/node を足さない）。
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

export function wasmBytes() {
  return readFileSync(join(import.meta.dirname, '..', '..', 'core-wasm', 'pkg', 'photo_curator_core_wasm_bg.wasm'))
}
