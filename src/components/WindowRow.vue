<script setup lang="ts">
import PinGlyph from "./PinGlyph.vue";
import type { WindowInfo } from "../types";

defineProps<{ item: WindowInfo; busy: boolean }>();
defineEmits<{ toggle: [hwnd: number] }>();

/** 标题为空的窗口（如部分无边框提示框）给一个可读的占位 */
const EMPTY_TITLE = "（无标题窗口）";
</script>

<template>
  <li class="row" :class="{ 'row--pinned': item.pinned }">
    <button
      class="pin"
      type="button"
      :aria-pressed="item.pinned"
      :aria-label="item.pinned ? `取消置顶 ${item.title}` : `置顶 ${item.title}`"
      :title="item.pinned ? '取消置顶' : '置顶'"
      :disabled="busy"
      @click="$emit('toggle', item.hwnd)"
    >
      <PinGlyph :filled="item.pinned" />
    </button>

    <span class="row__title" :title="item.title || EMPTY_TITLE">
      {{ item.title || EMPTY_TITLE }}
    </span>

    <span class="row__data">
      <span class="row__proc mono" :title="item.processName">{{
        item.processName
      }}</span>
      <span class="row__class mono" :title="item.className">{{
        item.className
      }}</span>
    </span>
  </li>
</template>

<style scoped>
.row {
  display: grid;
  grid-template-columns: 34px minmax(0, 1fr) auto;
  align-items: center;
  gap: 0 10px;
  min-height: 42px;
  padding: 0 16px;
  border-bottom: 1px solid var(--rule);
  background: var(--paper);
  transition: background var(--dur) var(--ease);
}
.row:hover {
  background: var(--well);
}

/* ── 反相 = 抬升。整套设计唯一的强调手段。 ───────────── */
.row--pinned {
  background: var(--ink);
  color: var(--on-ink);
  border-bottom-color: #262626;
}
.row--pinned:hover {
  background: #1c1c1c;
}

.pin {
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
.pin:hover:not(:disabled) {
  background: var(--well-hover);
  color: var(--ink);
}
.pin:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.row--pinned .pin {
  color: var(--on-ink);
}
.row--pinned .pin:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.16);
}

.row__title {
  min-width: 0;
  font-size: 13.5px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.row--pinned .row__title {
  font-weight: 600;
}

.row__data {
  display: flex;
  align-items: baseline;
  gap: 14px;
  min-width: 0;
}
.row__proc {
  font-size: 12px;
  color: var(--ink-2);
  max-width: 185px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.row__class {
  font-size: 11px;
  color: var(--ink-3);
  max-width: 205px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.row--pinned .row__proc {
  color: var(--on-ink);
}
.row--pinned .row__class {
  color: var(--on-ink-muted);
}

/* 窄窗口：数据列折到标题下方，不横向滚动 */
@media (max-width: 880px) {
  .row {
    grid-template-columns: 34px minmax(0, 1fr);
    padding-top: 8px;
    padding-bottom: 8px;
  }
  .row__data {
    grid-column: 2;
    flex-wrap: wrap;
    gap: 4px 12px;
  }
  .row__proc,
  .row__class {
    max-width: none;
  }
}
</style>
