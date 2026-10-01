<script setup lang="ts">
import type { SidecarProgress } from '~/lib/core'

/**
 * NAS（サイドカー）とこの端末の選別状況が、両方とも進んでいて違うときの 5 択（設計書 §4.6・U34）。
 * 既定（強調）のボタンは付けない。進んでいる側に印を付けるだけで、どれを選ぶかは人が決める。
 */
const {
  sidecarBusy,
  sidecarClash,
  sidecarMessage,
  resolveSidecarClash
} = useCurator()

// 端末の時計はずれるので、日時には必ず端末名を添える（新旧の判定には使わない）。
const formatDate = (value: number) =>
  new Intl.DateTimeFormat('ja-JP', { dateStyle: 'medium', timeStyle: 'short' }).format(value)

/** 1 行の要約: ★1 以上・ROUND と途中／完了・決めた組・残り・手直し・境目。 */
function describe(progress: SidecarProgress): string {
  if (!progress.started) return '選別なし（未着手）'
  const parts = [`★1 以上 ${progress.starred.toLocaleString()} 枚`]
  if (progress.round > 0) {
    parts.push(progress.finished
      ? `ROUND ${progress.round} 完了`
      : `ROUND ${progress.round} の途中（${progress.decided.toLocaleString()} 組決定・${progress.remaining.toLocaleString()} 枚残り）`)
  }
  if (progress.overrides > 0) parts.push(`手直し ${progress.overrides} か所`)
  if (progress.learned) parts.push('連写の境目を学習済み')
  return parts.join('・')
}

const reasonText = computed(() => {
  const clash = sidecarClash.value
  if (!clash) return ''
  switch (clash.reason) {
    case 'TheirsRestarted':
      return `${clash.theirsName} で最初からやり直されています。`
    case 'MineRestarted':
      return 'この端末で最初からやり直したあと、NAS の側で選別が進んでいます。'
    case 'ExtrasConflict':
      return '★と選別の途中は同じですが、連写の手直しか学習した境目が違います。'
    default: {
      const ahead = clash.order === 'Ahead'
        ? 'この端末の方が進んでいます。'
        : clash.order === 'Behind' ? 'NAS の方が進んでいます。' : ''
      return `どちらでも選別が進んでいて、中身が違います。${ahead}`
    }
  }
})

const canWrite = computed(() => sidecarClash.value?.access === 'readwrite')
/** 混ぜる 2 つは、片方の★1 以上が 0 枚なら出さない（積集合は全部消え、和集合は取り込む・書き込むと同じ）。 */
const canMerge = computed(() => {
  const preview = sidecarClash.value?.preview
  return canWrite.value && !!preview && preview.mine_starred > 0 && preview.theirs_starred > 0
})
</script>

<template>
<v-dialog :model-value="!!sidecarClash" max-width="600" persistent scrollable>
  <v-card v-if="sidecarClash" title="NAS の記録と、この端末の記録が違います">
    <v-card-text class="pt-4">
      <p class="text-body-2 mb-4">{{ reasonText }}</p>
      <div class="d-flex flex-column ga-2 mb-4">
        <div class="d-flex align-start ga-2">
          <v-icon :icon="sidecarClash.order === 'Ahead' ? 'mdi-arrow-up-bold-circle-outline' : 'mdi-circle-small'" size="small" class="mt-1" />
          <div class="text-body-2">
            <strong>この端末（{{ sidecarClash.mineName }}）</strong>: {{ describe(sidecarClash.mineProgress) }}
            <span v-if="sidecarClash.order === 'Ahead'" class="text-caption text-medium-emphasis">（こちらが進んでいます）</span>
          </div>
        </div>
        <div class="d-flex align-start ga-2">
          <v-icon :icon="sidecarClash.order === 'Behind' ? 'mdi-arrow-up-bold-circle-outline' : 'mdi-circle-small'" size="small" class="mt-1" />
          <div class="text-body-2">
            <strong>NAS（{{ sidecarClash.theirsName }}・{{ formatDate(sidecarClash.theirsAt) }}）</strong>: {{ describe(sidecarClash.theirsProgress) }}
            <span v-if="sidecarClash.order === 'Behind'" class="text-caption text-medium-emphasis">（こちらが進んでいます）</span>
          </div>
        </div>
      </div>

      <div class="d-flex flex-column ga-3">
        <div>
          <v-btn block variant="outlined" :disabled="sidecarBusy" @click="resolveSidecarClash('theirs')">サイドカーから取り込む</v-btn>
          <div class="text-caption text-medium-emphasis mt-1">この端末の選別状況を NAS の記録に置き換えます。</div>
        </div>
        <div>
          <v-btn block variant="outlined" :disabled="sidecarBusy" @click="resolveSidecarClash('keep')">この端末の状況を残す</v-btn>
          <div class="text-caption text-medium-emphasis mt-1">NAS には書かず、この端末だけで続けます。NAS の記録がまた変わったら、もう一度聞きます。</div>
        </div>
        <div v-if="canWrite">
          <v-btn block variant="outlined" :disabled="sidecarBusy" @click="resolveSidecarClash('mine')">この端末の状況をサイドカーに書き込む</v-btn>
          <div class="text-caption text-medium-emphasis mt-1">NAS の記録をこの端末の選別状況に置き換えます。ほかの端末は次に開いたときに取り込みます。</div>
        </div>
        <template v-if="canMerge">
          <div>
            <v-btn block variant="outlined" :disabled="sidecarBusy" @click="resolveSidecarClash('intersection')">両方で残した写真のみにする</v-btn>
            <div class="text-caption text-medium-emphasis mt-1">
              ★1 以上が {{ sidecarClash.preview.intersection_starred.toLocaleString() }} 枚になります（この端末 {{ sidecarClash.preview.mine_starred.toLocaleString() }}・NAS {{ sidecarClash.preview.theirs_starred.toLocaleString() }}）。片方でまだ見ていない写真は、見た側の★を使います。
            </div>
          </div>
          <div>
            <v-btn block variant="outlined" :disabled="sidecarBusy" @click="resolveSidecarClash('union')">どちらかで残した写真をすべて残す</v-btn>
            <div class="text-caption text-medium-emphasis mt-1">★1 以上が {{ sidecarClash.preview.union_starred.toLocaleString() }} 枚になります。</div>
          </div>
          <p v-if="sidecarClash.preview.mid_round" class="text-caption text-medium-emphasis mb-0">
            混ぜる 2 つは、途中の ROUND を終わりにします。まだ見ていない {{ sidecarClash.preview.undecided.toLocaleString() }} 枚は今の★のままになります。続きは結果画面の「もう一度選別する」から始めます。
          </p>
        </template>
      </div>

      <p class="text-caption text-medium-emphasis mt-4 mb-0">
        どれを選んでも、元の記録は消しません。置き換える前の記録は、写真のフォルダの <code>.photo-curator/</code> に <code>catalog.＜端末の id の先頭 12 文字＞.json</code> として残します。
      </p>
      <v-alert v-if="sidecarMessage" type="error" variant="tonal" density="comfortable" class="mt-4">{{ sidecarMessage }}</v-alert>
      <v-progress-linear v-if="sidecarBusy" indeterminate class="mt-4" />
    </v-card-text>
  </v-card>
</v-dialog>
</template>
