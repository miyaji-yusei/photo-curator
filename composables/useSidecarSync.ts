// サイドカーの4通り（設計02章 sidecarConflictDialog・03章）。
// 開いたときに Settled/Push/Pull/Clash を判定し、Push・Pull は自動で片付け、
// Clash だけ呼び出し側にダイアログを出させる（写真1枚ずつは選ばせない）。
import { ref } from 'vue'
import { useBackend } from '~/composables/useBackend'
import { useSnackbar } from '~/composables/useSnackbar'
import * as core from '~/lib/core'
import type { Sidecar } from '~/lib/core'
import type { Project } from '~/types/project'

export function useSidecarSync() {
  const backend = useBackend()
  const { show } = useSnackbar()
  // Clash のときだけ埋まる。埋まっている間、呼び出し側は sidecarConflictDialog を出す。
  const clash = ref<Sidecar | null>(null)

  /** プロジェクトを開いたときに呼ぶ。Pull は自動で取り込み、Push は黙って書く。 */
  async function checkOnOpen(project: Project): Promise<void> {
    if (!backend.sidecarSupported(project)) return
    // sidecar_decide は core（wasm）を呼ぶ。index.vue はここまで core を使わないので、
    // 呼ぶ側に初期化を強いず、ここで確実に済ませる。
    await core.init()
    const sync = await backend.checkSidecar(project)
    if (sync === 'Settled') return
    if (sync === 'Push') {
      await backend.pushSidecarIfChanged(project)
      show('この端末の結果を保存しました')
      return
    }
    if (typeof sync === 'object' && 'Pull' in sync) {
      await backend.adoptSidecar(project, sync.Pull)
      show('NAS の記録から続きを取り込みました')
      return
    }
    if (typeof sync === 'object' && 'Clash' in sync) {
      clash.value = sync.Clash
    }
  }

  /** 「この端末の結果を使う」。相手の分は catalog.<端末>.json に退避してから、自分の分を書く。 */
  async function keepMine(project: Project): Promise<void> {
    const theirs = clash.value
    if (!theirs) return
    clash.value = null
    await backend.keepMineSidecar(project, theirs)
    show('この端末の結果を NAS に保存しました')
  }

  /** 「NAS の記録を使う」。自分の分を捨てて、相手の分を取り込む。 */
  async function keepTheirs(project: Project): Promise<void> {
    const theirs = clash.value
    if (!theirs) return
    clash.value = null
    await backend.adoptSidecar(project, theirs)
    show('NAS の記録を取り込みました')
  }

  return { clash, checkOnOpen, keepMine, keepTheirs }
}

/**
 * 書き時（設計02章「書き時4つ」）: ラウンド完了は呼ぶ側（cull.vue）が個別に呼ぶ。
 * ここは残り3つ——画面を離れる・背面へ回る（visibilitychange）・窓を閉じる
 * （beforeunload）——をまとめて登録する。onUnmounted で呼び出し側が解除する。
 */
export function registerAutoPush(getProject: () => Project | null): () => void {
  const backend = useBackend()
  function push() {
    const project = getProject()
    if (project) void backend.pushSidecarIfChanged(project)
  }
  function onVisibility() {
    if (document.visibilityState === 'hidden') push()
  }
  document.addEventListener('visibilitychange', onVisibility)
  window.addEventListener('beforeunload', push)
  window.addEventListener('pagehide', push)
  return () => {
    // 画面を離れるときも書く（Android の DisposableEffect と同じ）。
    push()
    document.removeEventListener('visibilitychange', onVisibility)
    window.removeEventListener('beforeunload', push)
    window.removeEventListener('pagehide', push)
  }
}
