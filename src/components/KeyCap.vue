<script setup lang="ts">
/** 把热键画成一排真实键帽 —— 它同时是一句说明和一个可记忆的物体。
 *
 *  `size="lg"` 用于空台面：那里热键是屏幕上唯一值得做的事。 */
withDefaults(
  defineProps<{ keys: string[]; off?: boolean; size?: "sm" | "lg" }>(),
  { size: "sm" },
);
</script>

<template>
  <span class="keycaps" :class="[`keycaps--${size}`, { 'keycaps--off': off }]">
    <template v-for="(k, i) in keys" :key="`${k}-${i}`">
      <span v-if="i > 0" class="kbd-plus" :class="{ 'kbd-plus--lg': size === 'lg' }"
        >+</span
      >
      <kbd class="kbd" :class="{ 'kbd--lg': size === 'lg' }">{{ k }}</kbd>
    </template>
  </span>
</template>

<style scoped>
.keycaps {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  vertical-align: middle;
}
.keycaps--off .kbd {
  border-style: dashed;
  color: var(--ink-3);
}
</style>
