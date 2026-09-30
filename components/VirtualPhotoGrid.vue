<script setup lang="ts" generic="T">
/**
 * 写真の格子の仮想化。**見えている行の前後 2 行だけ描く**（4,000 枚でも DOM のタイルは 200 以下）。
 *
 * - 列数は `columns`（旧版の密度 `3 | 5 | 8 | 'auto'`）。`'auto'` は CSS の
 *   `repeat(auto-fill, minmax(minTile, 1fr))` と同じ数になるよう、幅から数える
 * - どの行も同じ高さ（タイルの縦横比が決まっているため）。最初に描いた 1 枚から測る
 * - スクロールするのは window でも、その中の `overflow: auto` の箱でもよい（親をさかのぼって探す）
 * - 末尾の 2 行が見えたら `end` を出す。呼び出し側がページ送りを続ける
 *
 * slot `default`: 1 タイル（`item`・`index`・`url`）。絵は `<img :src="url" loading="lazy" decoding="async">`。
 */
type Columns = 3 | 5 | 8 | 'auto'

const props = withDefaults(defineProps<{
  items: readonly T[]
  /** 1 枚の鍵（v-for の key）。 */
  itemKey: (item: T) => string
  /** 1 枚の絵の URL。 */
  itemUrl?: (item: T) => string
  columns?: Columns
  gap?: number
  /** `columns='auto'` のときの 1 タイルの最小幅。 */
  minTile?: number
  /** 前後に余分に描く行数。 */
  overscanRows?: number
  /** 高さを測る前の 1 タイルの高さの見積もり（幅に対する比）。 */
  tileAspect?: number
}>(), {
  itemUrl: undefined,
  columns: 'auto',
  gap: 14,
  minTile: 160,
  overscanRows: 2,
  tileAspect: 1
})

const emit = defineEmits<{ end: [] }>()

const root = ref<HTMLElement | null>(null)
const inner = ref<HTMLElement | null>(null)
const width = ref(0)
const rowHeight = ref(0)
/** 格子の上端が、スクロールする箱の上端からどれだけ下にあるか（負なら格子の途中まで進んでいる）。 */
const scrolled = ref(0)
const viewport = ref(800)

const columnCount = computed(() => {
  if (props.columns !== 'auto') return props.columns
  return Math.max(1, Math.floor((width.value + props.gap) / (props.minTile + props.gap)))
})
const tileWidth = computed(() => (width.value - props.gap * (columnCount.value - 1)) / columnCount.value)
/** 測る前は、幅から見積もる。 */
const estimatedRow = computed(() => Math.max(40, Math.round(tileWidth.value * props.tileAspect)))
const stride = computed(() => (rowHeight.value || estimatedRow.value) + props.gap)
const rowCount = computed(() => Math.ceil(props.items.length / columnCount.value))

const range = computed(() => {
  const rows = rowCount.value
  if (!rows) return { first: 0, last: -1 }
  const top = Math.max(0, Math.floor(scrolled.value / stride.value))
  const bottom = Math.max(0, Math.floor((scrolled.value + viewport.value) / stride.value))
  return {
    first: Math.max(0, Math.min(rows - 1, top) - props.overscanRows),
    last: Math.min(rows - 1, bottom + props.overscanRows)
  }
})

const visible = computed(() => {
  const { first, last } = range.value
  const cols = columnCount.value
  const out: { item: T, index: number }[] = []
  for (let index = first * cols; index < Math.min(props.items.length, (last + 1) * cols); index++) {
    out.push({ item: props.items[index]!, index })
  }
  return out
})

const padTop = computed(() => range.value.first * stride.value)
const padBottom = computed(() => Math.max(0, (rowCount.value - 1 - range.value.last) * stride.value))

watch(
  () => [range.value.last, rowCount.value] as const,
  ([last, rows]) => { if (rows && last >= rows - 1 - props.overscanRows) emit('end') },
  { flush: 'post' }
)

// ---- スクロールの位置 ----------------------------------------------------

let scroller: HTMLElement | Window | null = null
let frame = 0

function scrollParent(el: HTMLElement | null): HTMLElement | Window {
  // body・html は window のスクロールなので、そこまで来たら window。
  for (let node = el?.parentElement ?? null; node && node !== document.body && node !== document.documentElement; node = node.parentElement) {
    const overflow = getComputedStyle(node).overflowY
    if (overflow === 'auto' || overflow === 'scroll') return node
  }
  return window
}

function measureScroll() {
  frame = 0
  const el = root.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  const scrollerTop = scroller instanceof HTMLElement ? scroller.getBoundingClientRect().top : 0
  viewport.value = scroller instanceof HTMLElement ? scroller.clientHeight : window.innerHeight
  // 格子の上端から見て、見えている窓の上端がどこか。
  scrolled.value = scrollerTop - rect.top
  width.value = el.clientWidth
  measureRow()
}

function requestMeasure() {
  if (!frame) frame = requestAnimationFrame(measureScroll)
}

/** 描いた 1 行の高さを測る（行の高さはどれも同じ）。 */
function measureRow() {
  const tile = inner.value?.firstElementChild as HTMLElement | null
  if (!tile) return
  const height = Math.round(tile.getBoundingClientRect().height)
  if (height > 0 && Math.abs(height - rowHeight.value) > 1) rowHeight.value = height
}

let observer: ResizeObserver | null = null
onMounted(() => {
  scroller = scrollParent(root.value)
  scroller.addEventListener('scroll', requestMeasure, { passive: true })
  window.addEventListener('resize', requestMeasure, { passive: true })
  if (typeof ResizeObserver !== 'undefined' && root.value) {
    observer = new ResizeObserver(requestMeasure)
    observer.observe(root.value)
  }
  measureScroll()
})
onBeforeUnmount(() => {
  scroller?.removeEventListener('scroll', requestMeasure)
  window.removeEventListener('resize', requestMeasure)
  observer?.disconnect()
  if (frame) cancelAnimationFrame(frame)
})
// 描くタイルが変わったら（列数の変更・ページ送り）、高さを測り直す。
watch([() => props.items.length, columnCount], () => nextTick(requestMeasure))
watch(columnCount, () => { rowHeight.value = 0 })
onUpdated(measureRow)
</script>

<template>
  <div ref="root" class="virtual-photo-grid">
    <div
      ref="inner"
      class="virtual-photo-grid__rows"
      :style="{
        display: 'grid',
        gridTemplateColumns: `repeat(${columnCount}, minmax(0, 1fr))`,
        gap: `${gap}px`,
        paddingTop: `${padTop}px`,
        paddingBottom: `${padBottom}px`
      }"
    >
      <template v-for="entry in visible" :key="itemKey(entry.item)">
        <slot :item="entry.item" :index="entry.index" :url="itemUrl ? itemUrl(entry.item) : ''" />
      </template>
    </div>
  </div>
</template>
