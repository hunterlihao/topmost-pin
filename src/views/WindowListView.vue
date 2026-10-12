<script setup lang="ts">
import EmptyDeck from "../components/EmptyDeck.vue";
import WindowRow from "../components/WindowRow.vue";
import { useAppStore } from "../stores/app";

const {
  state: s,
  pinnedWindows,
  looseWindows,
  searchActive,
  visibleCount,
  hotkeyKeys,
  refresh,
  toggle,
  unpinAll,
  setQuery,
} = useAppStore();

function onSearch(e: Event) {
  setQuery((e.target as HTMLInputElement).value);
}
</script>

<template>
  <section class="view">
    <!-- ── 台面：已置顶 ────────────────────────────────────────────────
         一块有边界的墨，独立滚动。这层「边界」是有意义的：z 序里被抬起来的
         东西不构成一个列表，它们构成**一个平面** —— 而且不会因为你翻找
         下面的窗口而移动。所以台面自己滚，下层候选区自己滚。 -->
    <div v-if="pinnedWindows.length > 0" class="deck">
      <div class="deck__head">
        <span class="deck__title">
          已置顶 <span class="deck__count mono">{{ pinnedWindows.length }}</span>
        </span>
        <button
          class="btn btn-sm deck__clear"
          type="button"
          :disabled="s.unpinning"
          @click="unpinAll()"
        >
          全部取消
        </button>
      </div>

      <ul class="deck__rows">
        <WindowRow
          v-for="w in pinnedWindows"
          :key="w.hwnd"
          :item="w"
          :busy="s.busy.has(w.hwnd)"
          variant="deck"
          @toggle="toggle"
        />
      </ul>
    </div>

    <!-- 空台面：界面 99% 时间停在这一屏，所以它自己得是被设计过的 -->
    <EmptyDeck
      v-else-if="!s.loading"
      :keys="hotkeyKeys"
      :hotkey-active="!!s.snapshot?.hotkeyActive"
    />

    <!-- ── 下层：候选窗口 ──────────────────────────────────────────── -->
    <div class="surface">
      <div class="toolbar">
        <input
          class="input toolbar__search"
          type="search"
          :value="s.query"
          placeholder="搜索窗口标题、进程名或类名"
          aria-label="搜索窗口"
          @input="onSearch"
        />
        <!-- 用户主动点刷新 = 明确表示要看最新状态，这时才清错误条 -->
        <button
          class="btn btn-sm toolbar__refresh"
          type="button"
          @click="refresh({ clearError: true })"
        >
          刷新
        </button>
      </div>

      <p v-if="s.loading" class="empty">正在读取窗口…</p>

      <p v-else-if="visibleCount === 0" class="empty">
        没有找到可见窗口。<br />
        <span class="empty__sub">窗口可能都被最小化了。</span>
      </p>

      <p v-else-if="looseWindows.length === 0" class="empty">
        <template v-if="searchActive">
          没有匹配的窗口。<br />
          <span class="empty__sub">换个关键词试试。</span>
        </template>
        <template v-else>
          所有可见窗口都已经置顶了。<br />
          <span class="empty__sub"
            >在台面上点图钉可以取消，或按热键切换。</span
          >
        </template>
      </p>

      <ul v-else class="list">
        <WindowRow
          v-for="w in looseWindows"
          :key="w.hwnd"
          :item="w"
          :busy="s.busy.has(w.hwnd)"
          variant="surface"
          @toggle="toggle"
        />
      </ul>
    </div>
  </section>
</template>

<style scoped>
.view {
  display: flex;
  flex-direction: column;
  min-height: 0;
  height: 100%;
}

/* ── 台面 ─────────────────────────────────────────── */

.deck {
  display: flex;
  flex-direction: column;
  flex: none;
  /* 有界：台面最多占列表区的 46%，再多就在自己的区域里滚。
     不给上限的话，置顶 20 个窗口会把下面的候选区挤到看不见。 */
  max-height: 46%;
  background: var(--ink);
  color: var(--on-ink);
  /* 2px 的水面线：抬起来的那一层到纸面之间的硬边。
     整个平面都是墨，所以这条线读起来是墨块的加厚下沿，而不是一条分隔线。 */
  border-bottom: 2px solid var(--ink);
}

.deck__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex: none;
  height: 30px;
  padding: 0 14px;
  border-bottom: 1px solid var(--deck-line);
}

.deck__title {
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.1em;
  color: var(--on-ink-2);
}
.deck__count {
  color: var(--on-ink);
  font-size: 12px;
}

/* 反相上下文里的小按钮：不能沿用纸面上的 .btn（白底黑字会很扎眼）。
   默认连边框都不给 —— 「全部取消」是台面上唯一会削弱内容的东西，
   它不该比它管的那几行更显眼；悬停时才浮出来。 */
.deck__clear {
  height: 22px;
  padding: 0 8px;
  border-color: transparent;
  background: transparent;
  color: var(--on-ink-2);
  font-size: 11.5px;
}
.deck__clear:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.14);
  border-color: rgba(255, 255, 255, 0.4);
  color: var(--on-ink);
}
.deck__clear:disabled {
  color: var(--on-ink-3);
  border-color: transparent;
}

.deck__rows {
  flex: 1;
  min-height: 0;
  margin: 0;
  padding: 0;
  overflow-y: auto;
  list-style: none;
}

/* 墨面上的滚动条要反相，否则深灰滑道压在黑底上看不见 */
.deck__rows::-webkit-scrollbar-thumb {
  background: #4a4a4a;
  border: 3px solid transparent;
  background-clip: content-box;
  border-radius: 999px;
}
.deck__rows::-webkit-scrollbar-thumb:hover {
  background: #6a6a6a;
  background-clip: content-box;
}

/* ── 下层候选区 ───────────────────────────────────── */

.surface {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}

.toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: none;
  padding: 10px 16px;
  border-bottom: 1px solid var(--rule);
}
.toolbar__search {
  flex: 1;
  min-width: 0;
}
.toolbar__refresh {
  flex: none;
}

.empty {
  margin: 0;
  padding: 48px 16px;
  text-align: center;
  color: var(--ink-2);
  font-size: 13.5px;
  line-height: 2;
}
.empty__sub {
  color: var(--ink-3);
  font-size: 12.5px;
}

.list {
  flex: 1;
  min-height: 0;
  margin: 0;
  padding: 0;
  overflow-y: auto;
  list-style: none;
}

@media (max-width: 720px) {
  .deck__head {
    padding: 0 12px;
  }
}
</style>
