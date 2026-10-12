<script setup lang="ts">
import { computed } from "vue";

import PinGlyph from "./PinGlyph.vue";
import type { WindowInfo } from "../types";

/**
 * 一行窗口。同一个组件两种形态：
 *
 *   deck    —— 台面（已置顶）上的行：反相、紧凑，整体是一块墨
 *   surface —— 下层候选区的行：纸面、宽松、发丝线分行
 *
 * 两种形态共用同一份字段与同一套列宽，差别只在明度与行高 ——
 * 因为「抬起来了」和「沉在下面」讲的是同一个对象在两个平面上，
 * 拆成两个组件只会让两边的字段慢慢长歪。
 */
const props = defineProps<{
  item: WindowInfo;
  busy: boolean;
  variant: "deck" | "surface";
}>();

defineEmits<{ toggle: [hwnd: number] }>();

/** 标题为空的窗口（如部分无边框提示框）给一个可读的占位 */
const EMPTY_TITLE = "（无标题窗口）";

const title = computed(() => props.item.title || EMPTY_TITLE);

/**
 * HWND 显示为定宽十六进制。
 *
 * 为什么要把它摆到界面上：同标题的窗口是常态（两个资源管理器、几个
 * 「新建标签页」），当标题、进程名、类名三者全都一样时，句柄是**唯一**
 * 能区分它们的字段。定宽 8 位是为了让这一列能竖着对齐地扫下来。
 *
 * `>>> 0` 取低 32 位：Win32 的用户对象句柄本来就是 32 位有效，
 * 而 Rust 侧按 isize 序列化，高位可能是符号扩展出来的。
 */
const hwndHex = computed(
  () =>
    "0x" + (props.item.hwnd >>> 0).toString(16).toUpperCase().padStart(8, "0"),
);
</script>

<template>
  <li class="row" :class="`row--${variant}`">
    <button
      class="row__pin"
      type="button"
      :aria-pressed="item.pinned"
      :aria-label="item.pinned ? `取消置顶 ${title}` : `置顶 ${title}`"
      :title="item.pinned ? '取消置顶' : '置顶'"
      :disabled="busy"
      @click="$emit('toggle', item.hwnd)"
    >
      <PinGlyph :filled="item.pinned" />
    </button>

    <!-- 三个字段合成一块，整体靠右。工具提示里补上类名与句柄，
         标题被截断时鼠标一悬就能看到全称。 -->
    <span
      class="row__title"
      :title="`${title}\n${item.processName}\n${item.className}`"
    >
      {{ title }}
    </span>

    <span class="row__data">
      <span class="row__proc mono" :title="item.processName">
        {{ item.processName }}
      </span>
      <span class="row__class mono" :title="item.className">
        {{ item.className }}
      </span>
      <span class="row__hwnd mono" :title="`窗口句柄 ${hwndHex}`">
        {{ hwndHex }}
      </span>
    </span>
  </li>
</template>

<style scoped>
/* ── 两个平面共用同一套列 ────────────────────────────
   台面和下层候选区用的是**同一个网格**：同样的左右留白、同样的列宽，
   差别只有行高、字号和明度。

   这不是为了省事。两个平面本来就是同一份窗口列表被 z 序切开的两半 ——
   列一对齐，眼睛就可以竖着从台面扫到下层去比对；列不对齐的话，
   它们看起来就成了两张互不相干的表。 */
.row {
  display: grid;
  grid-template-columns: 30px minmax(0, 1fr) auto;
  align-items: center;
  gap: 0 12px;
  padding: 0 16px;
  border-bottom: 1px solid var(--rule);
}

.row__title {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 定宽列 —— 等宽字面在这里的用处就是**竖着对齐**：
   进程名 / 类名 / 句柄各自成一列，可以一列一列往下扫。 */
.row__data {
  display: flex;
  align-items: baseline;
  gap: 0 12px;
}
.row__proc,
.row__class,
.row__hwnd {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.row__proc {
  width: 158px;
}
.row__class {
  width: 166px;
}
.row__hwnd {
  width: 92px;
  text-align: right;
}

/* ── 下层候选区：纸面、宽松 ─────────────────────── */

.row--surface {
  min-height: 44px;
  background: var(--paper);
  transition: background var(--dur) var(--ease);
}
.row--surface:hover {
  background: var(--well);
}
.row--surface .row__title {
  font-size: 13.5px;
  font-weight: 500;
}
.row--surface .row__proc {
  font-size: 12px;
  color: var(--ink-2);
}
.row--surface .row__class,
.row--surface .row__hwnd {
  font-size: 11.5px;
  color: var(--ink-3);
}

/* ── 台面（已置顶）：反相、紧凑 ──────────────────
   反相 = 抬升。整套设计唯一的强调手段：整个平面是一块墨，
   行与行的分隔是墨面上更深一档的线，而不是纸面上更浅一档的线。 */

.row--deck {
  min-height: 38px;
  border-bottom-color: var(--deck-line);
  background: var(--ink);
  color: var(--on-ink);
  transition: background var(--dur) var(--ease);
}
.row--deck:hover {
  background: var(--deck-hover);
}
.row--deck .row__title {
  font-size: 12.5px;
  font-weight: 600;
}
.row--deck .row__proc {
  font-size: 11.5px;
  color: var(--on-ink-2);
}
.row--deck .row__class,
.row--deck .row__hwnd {
  font-size: 11px;
  color: var(--on-ink-3);
}

/* ── 图钉按钮 ───────────────────────────────────── */

.row__pin {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  padding: 0;
  border: 0;
  border-radius: var(--radius-object);
  background: transparent;
  color: var(--ink-2);
  transition: background var(--dur) var(--ease), color var(--dur) var(--ease);
}
.row__pin:hover:not(:disabled) {
  background: var(--well-hover);
  color: var(--ink);
}
.row__pin:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
.row--deck .row__pin {
  color: var(--on-ink);
}
.row--deck .row__pin:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.16);
  color: var(--on-ink);
}

/* ── 窄窗 ─────────────────────────────────────────
   类名最宽、也最不关键，先砍它；再窄就把数据块折到标题下方，
   全程不出现横向滚动。两个平面同步收，断点保持一致。 */

@media (max-width: 900px) {
  .row__class {
    display: none;
  }
  .row__proc {
    width: 146px;
  }
}

@media (max-width: 780px) {
  .row {
    grid-template-columns: 30px minmax(0, 1fr);
    padding-top: 7px;
    padding-bottom: 7px;
    row-gap: 3px;
  }
  .row__data {
    grid-column: 2;
    gap: 0 12px;
  }
  .row__proc,
  .row__hwnd {
    width: auto;
  }
  .row__hwnd {
    text-align: left;
  }
}
</style>
