<script setup lang="ts">
/**
 * 错误条。整个界面没有彩色，因此这里的「被注意」由三重量级承担：
 * 2px 墨色边框 + 反相的前置标记块 + 加粗文案。
 */
defineProps<{ message: string }>();
defineEmits<{ dismiss: [] }>();
</script>

<template>
  <div class="strip" role="alert">
    <span class="strip__mark" aria-hidden="true">!</span>
    <span class="strip__text">{{ message }}</span>
    <button
      class="strip__close"
      type="button"
      aria-label="关闭提示"
      title="关闭"
      @click="$emit('dismiss')"
    >
      ✕
    </button>
  </div>
</template>

<style scoped>
.strip {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  align-items: stretch;
  margin: 0;
  border: 2px solid var(--ink);
  background: var(--paper);
}
.strip__mark {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  background: var(--ink);
  color: var(--on-ink);
  font-family: var(--font-data);
  font-size: 15px;
  font-weight: 700;
}
.strip__text {
  padding: 9px 12px;
  font-size: 13px;
  line-height: 1.5;
}
.strip__close {
  padding: 0 11px;
  border: 0;
  border-left: 1px solid var(--rule);
  background: transparent;
  color: var(--ink-3);
  font-size: 12px;
}
.strip__close:hover {
  background: var(--well);
  color: var(--ink);
}
</style>
