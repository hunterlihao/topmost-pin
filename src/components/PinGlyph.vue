<script setup lang="ts">
/**
 * 图钉字形 —— 状态的第二编码。
 *
 * 置顶 = 实心，未置顶 = 描边。同一条路径两种渲染，即使有人完全无法
 * 分辨明度差异，填充与描边的形状区别仍然可读。
 */
defineProps<{ filled: boolean }>();

const PIN_PATH =
  "M16 9V4h1c.55 0 1-.45 1-1s-.45-1-1-1H7c-.55 0-1 .45-1 1s.45 1 1 1h1v5c0 1.66-1.34 3-3 3v2h5.97v7l1 1 1-1v-7H19v-2c-1.66 0-3-1.34-3-3z";
</script>

<template>
  <svg
    class="glyph"
    viewBox="0 0 24 24"
    width="15"
    height="15"
    aria-hidden="true"
    focusable="false"
  >
    <path
      :d="PIN_PATH"
      :fill="filled ? 'currentColor' : 'none'"
      :stroke="filled ? 'none' : 'currentColor'"
      stroke-width="1.7"
      stroke-linejoin="round"
    />
  </svg>
</template>

<style scoped>
.glyph {
  display: block;
  flex: none;
}
</style>
