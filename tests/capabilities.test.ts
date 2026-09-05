import { describe, expect, it } from 'vitest'
import { capabilitiesFor, detectPlatform } from '~/utils/capabilities'

/** 実機・実環境の User-Agent を模したもの。 */
const UA = {
  windows: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/120.0',
  // Galaxy Z Fold の WebView。`Android` を必ず含む。
  android: 'Mozilla/5.0 (Linux; Android 16; SM-F971C) AppleWebKit/537.36 Chrome/140.0 Mobile',
  ipad: 'Mozilla/5.0 (iPad; CPU OS 18_0 like Mac OS X) AppleWebKit/605.1.15 Safari/604.1'
}

describe('detectPlatform', () => {
  it('Tauri でなければ、UA によらずブラウザ', () => {
    expect(detectPlatform(false, UA.ipad)).toBe('browser')
    expect(detectPlatform(false, UA.windows)).toBe('browser')
    // Android の Chrome で Web 版を開いた場合。**アプリではない。**
    expect(detectPlatform(false, UA.android)).toBe('browser')
  })

  it('Tauri かつ Android の UA なら android', () => {
    expect(detectPlatform(true, UA.android)).toBe('android')
  })

  it('Tauri で Android でなければ desktop', () => {
    expect(detectPlatform(true, UA.windows)).toBe('desktop')
  })

  it('UA が空でも desktop に落ちる（Android と誤判定しない）', () => {
    expect(detectPlatform(true, '')).toBe('desktop')
  })
})

describe('capabilitiesFor', () => {
  it('Android はフォルダを走査できるが、キーボードと大きなグループは持たない', () => {
    const caps = capabilitiesFor('android')
    expect(caps.browseFolders).toBe(true)
    expect(caps.keyboard).toBe(false)
    expect(caps.largeGroups).toBe(false)
  })

  it('デスクトップだけが原本の XMP を書き換えられる', () => {
    expect(capabilitiesFor('desktop').writeMetadata).toBe(true)
    expect(capabilitiesFor('android').writeMetadata).toBe(false)
    expect(capabilitiesFor('browser').writeMetadata).toBe(false)
  })

  it('ブラウザだけがフォルダを走査できない', () => {
    expect(capabilitiesFor('browser').browseFolders).toBe(false)
    expect(capabilitiesFor('desktop').browseFolders).toBe(true)
    expect(capabilitiesFor('android').browseFolders).toBe(true)
  })

  it('ブラウザはリロードで原本を失うので、フル解像度を出せない', () => {
    expect(capabilitiesFor('browser').fullResolution).toBe(false)
    expect(capabilitiesFor('android').fullResolution).toBe(true)
  })

  it('デスクトップだけが大きなグループを持つ', () => {
    expect(capabilitiesFor('desktop').largeGroups).toBe(true)
    expect(capabilitiesFor('android').largeGroups).toBe(false)
    expect(capabilitiesFor('browser').largeGroups).toBe(false)
  })
})
