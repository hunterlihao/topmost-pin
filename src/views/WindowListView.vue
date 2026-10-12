<script setup lang="ts">
import WindowRow from "../components/WindowRow.vue";
import KeyCap from "../components/KeyCap.vue";
import { useAppStore } from "../stores/app";

const {
  state: s,
  filtered,
  pinnedWindows,
  looseWindows,
  pinnedCount,
  visibleCount,
  hotkeyKeys,
  refresh,
  toggle,
  unpinAll,
  setQuery,
  setFilter,
} = useAppStore();

function onSearch(e: Event) {
  setQuery((e.target as HTMLInputElement).value);
}
</script>

<template>
  <section class="view">
    <!-- 工具条：安静，只做分区与发丝线 -->
    <div class="toolbar">
      <input
        class="input toolbar__search"
        type="search"
        :value="s.query"
        placeholder="搜索窗口标题或进程名"
        aria-label="搜索窗口"
        @input="onSearch"
      />
      <div class="seg" role="group" aria-label="筛选">
        <button
          type="button"
          :aria-pressed="s.filter === 'all'"
          @click="setFilter('all')"
        >
          全部
        </button>
        <button
          type="button"
          :aria-pressed="s.filter === 'pinned'"
          @click="setFilter('pinned')"
        >
          已置顶
        </button>
      </div>
      <button class="btn btn-sm" type="button" @click="refresh()">刷新</button>
      <button
        class="btn btn-sm"
        type="button"
        :disabled="s.unpinning || pinnedCount === 0"
        @click="unpinAll()"
      >
        全部取消
      </button>
    </div>

    <!-- 一个窗口都没置顶时，把热键摆在这里 —— 它是最该被学会的那件事 -->
    <p v-if="pinnedCount === 0 && !s.loading" class="hint">
      还没有置顶窗口 —— 指向任意窗口按
      <KeyCap :keys="hotkeyKeys" :off="!s.snapshot?.hotkeyActive" />
      ，或点下方窗口行左侧的图钉。
    </p>

    <p v-if="s.loading" class="empty">正在读取窗口…</p>

    <p v-else-if="visibleCount === 0" class="empty">
      没有找到可见窗口。<br />
      <span class="empty__sub">窗口可能都被最小化了。</span>
    </p>

    <p v-else-if="filtered.length === 0" class="empty">
      没有匹配的窗口。<br />
      <span class="empty__sub">换个关键词，或切回「全部」。</span>
    </p>

    <ul v-else class="list">
      <WindowRow
        v-for="w in pinnedWindows"
        :key="w.hwnd"
        :item="w"
        :busy="s.busy.has(w.hwnd)"
        @toggle="toggle"
      />

      <!-- 水面线：2px 粗墨线，之上是被抬起的窗口 -->
      <li
        v-if="pinnedWindows.length > 0 && looseWindows.length > 0"
        class="waterline"
        aria-hidden="true"
      ></li>

      <WindowRow
        v-for="w in looseWindows"
        :key="w.hwnd"
        :item="w"
        :busy="s.busy.has(w.hwnd)"
        @toggle="toggle"
      />
    </ul>
  </section>
</template>

<style scoped>
.view {
  display: flex;
  flex-direction: column;
  min-height: 0;
  height: 100%;
}

.toolbar {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto auto auto;
  align-items: center;
  gap: 8px;
  padding: 10px 16px;
  border-bottom: 1px solid var(--rule);
}

.hint {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  margin: 0;
  padding: 10px 16px;
  border-bottom: 1px solid var(--rule);
  background: var(--well);
  color: var(--ink-2);
  font-size: 12.5px;
}

.empty {
  margin: 0;
  padding: 56px 16px;
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

/* 水面线：2px 纯墨，把「抬起的」与「沉底的」分开 */
.waterline {
  height: 2px;
  background: var(--ink);
}

@media (max-width: 720px) {
  .toolbar {
    grid-template-columns: minmax(0, 1fr) auto;
    row-gap: 8px;
  }
}
</style>
