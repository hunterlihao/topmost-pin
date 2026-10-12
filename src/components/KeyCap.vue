<script setup lang="ts">
/** 把热键画成一排真实键帽 —— 它同时是一句说明和一个可记忆的物体。 */
defineProps<{ keys: string[]; off?: boolean }>();
</script>

<template>
  <span class="keycaps" :class="{ 'keycaps--off': off }">
    <template v-for="(k, i) in keys" :key="`${k}-${i}`">
      <span v-if="i > 0" class="kbd-plus">+</span>
      <kbd class="kbd">{{ k }}</kbd>
    </template>
  </span>
</template>

<style scoped>
.keycaps {
  display: inline-flex;
  align-items: center;
  gap: 2px;
}
.keycaps--off .kbd {
  border-style: dashed;
  color: var(--ink-3);
}
</style>
