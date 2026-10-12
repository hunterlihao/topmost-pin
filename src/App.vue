<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";

import * as api from "./api";
import ErrorStrip from "./components/ErrorStrip.vue";
import KeyCap from "./components/KeyCap.vue";
import SettingsView from "./views/SettingsView.vue";
import WindowListView from "./views/WindowListView.vue";
import { useAppStore } from "./stores/app";

const {
  state: s,
  hotkeyKeys,
  pinnedCount,
  visibleCount,
  refresh,
  dismissError,
  setView,
  init,
} = useAppStore();

/* 窗口重新获得焦点时刷新一次；聚焦期间每 5s 轻量轮询。
   轮询只在有焦点时进行，后台不消耗。 */
const POLL_MS = 5000;
let timer: number | undefined;

function onFocus() {
  void refresh();
}

onMounted(async () => {
  // 等第一帧真的画出来再通知后端显示主窗口。
  // 用两级 requestAnimationFrame：第一级在绘制前触发，第二级才落在首次绘制之后。
  // 少了这一步，主窗口会先以白底暴露一瞬。
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      void api.mainReady();
    });
  });

  await init();
  window.addEventListener("focus", onFocus);
  timer = window.setInterval(() => {
    if (document.hasFocus()) void refresh();
  }, POLL_MS);
});

onUnmounted(() => {
  window.removeEventListener("focus", onFocus);
  if (timer !== undefined) window.clearInterval(timer);
});
</script>

<template>
  <div class="app">
    <header class="masthead">
      <div class="masthead__id">
        <h1 class="masthead__title">窗口置顶助手</h1>
        <nav class="seg" aria-label="视图">
          <button
            type="button"
            :aria-pressed="s.view === 'windows'"
            @click="setView('windows')"
          >
            窗口
          </button>
          <button
            type="button"
            :aria-pressed="s.view === 'settings'"
            @click="setView('settings')"
          >
            设置
          </button>
        </nav>
      </div>

      <div class="masthead__meta">
        <KeyCap :keys="hotkeyKeys" :off="!s.snapshot?.hotkeyActive" />
        <span class="masthead__divider" aria-hidden="true"></span>
        <span class="label">已置顶</span>
        <span
          class="count mono"
          :title="`已置顶 ${pinnedCount}，可见窗口 ${visibleCount}`"
        >
          <strong class="count__now">{{ pinnedCount }}</strong>
          <span class="count__of">/ {{ visibleCount }}</span>
        </span>
      </div>
    </header>

    <ErrorStrip
      v-if="s.error"
      class="app__error"
      :message="s.error"
      @dismiss="dismissError"
    />

    <main class="app__body">
      <WindowListView v-if="s.view === 'windows'" />
      <SettingsView v-else />
    </main>

    <Transition name="toast">
      <div v-if="s.toast" class="toast mono" role="status">{{ s.toast }}</div>
    </Transition>
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--paper);
}

/* ── 报头 ─────────────────────────────────────────── */

.masthead {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  flex: none;
  height: 52px;
  padding: 0 16px;
  border-bottom: 1px solid var(--rule);
}

.masthead__id {
  display: flex;
  align-items: center;
  gap: 16px;
  min-width: 0;
}

.masthead__title {
  font-family: var(--font-display);
  font-size: 16px;
  font-weight: 700;
  letter-spacing: -0.02em;
  white-space: nowrap;
}

.masthead__meta {
  display: flex;
  align-items: center;
  gap: 10px;
}

.masthead__divider {
  width: 1px;
  height: 20px;
  background: var(--rule);
}

.count {
  display: inline-flex;
  align-items: baseline;
  gap: 4px;
  font-size: 13px;
}
.count__now {
  font-weight: 700;
}
.count__of {
  color: var(--ink-3);
}

.app__error {
  flex: none;
  margin: 12px 16px 0;
}

.app__body {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}

/* ── Toast ────────────────────────────────────────── */

.toast {
  position: fixed;
  left: 50%;
  bottom: 18px;
  transform: translateX(-50%);
  padding: 8px 14px;
  border-radius: var(--radius-object);
  background: var(--ink);
  color: var(--on-ink);
  font-size: 12.5px;
  box-shadow: var(--shadow-float);
  pointer-events: none;
  z-index: 20;
}

.toast-enter-active,
.toast-leave-active {
  transition: opacity var(--dur) var(--ease),
    transform var(--dur) var(--ease);
}
.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(6px);
}

@media (max-width: 720px) {
  .masthead {
    height: auto;
    flex-wrap: wrap;
    gap: 10px;
    padding: 10px 16px;
  }
}
</style>
