/** 应用状态。单一 reactive 实例，组件通过 useAppStore() 取用。 */

import { computed, reactive, readonly } from "vue";

import * as api from "../api";
import type { Snapshot, WindowInfo } from "../types";

export type FilterMode = "all" | "pinned";
export type ViewName = "windows" | "settings";

const state = reactive({
  windows: [] as WindowInfo[],
  snapshot: null as Snapshot | null,
  loading: true,
  /* 行级操作中的 HWND，用于禁用按钮并显示忙碌态 */
  busy: new Set<number>(),
  unpinning: false,
  query: "",
  filter: "all" as FilterMode,
  view: "windows" as ViewName,
  error: "" as string,
  toast: "" as string,
});

let toastTimer: number | undefined;

/** 归一化：去掉首尾空白并折叠内部连续空白，便于标题匹配 */
function normalize(s: string) {
  return s.trim().replace(/\s+/g, " ");
}

const filtered = computed(() => {
  const q = normalize(state.query).toLowerCase();
  return state.windows.filter((w) => {
    if (state.filter === "pinned" && !w.pinned) return false;
    if (!q) return true;
    return (
      w.title.toLowerCase().includes(q) ||
      w.processName.toLowerCase().includes(q) ||
      w.className.toLowerCase().includes(q)
    );
  });
});

/** 置顶的排在上面 —— 这正是「水面线之上」 */
const pinnedWindows = computed(() => filtered.value.filter((w) => w.pinned));
const looseWindows = computed(() => filtered.value.filter((w) => !w.pinned));

const pinnedCount = computed(() => state.windows.filter((w) => w.pinned).length);
const visibleCount = computed(() => state.windows.length);

const hotkeyKeys = computed(() => {
  const hk = state.snapshot?.hotkey ?? "";
  return hk ? hk.split("+").map((k) => k.trim()).filter(Boolean) : [];
});

function showToast(message: string) {
  state.toast = message;
  if (toastTimer) window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    state.toast = "";
  }, 4000);
}

function dismissError() {
  state.error = "";
}

/** 拉取窗口列表与快照 */
async function refresh() {
  try {
    const [windows, snapshot] = await Promise.all([
      api.listWindows(),
      api.getSnapshot(),
    ]);
    state.windows = windows;
    state.snapshot = snapshot;
    state.error = "";
  } catch (e) {
    state.error = `无法读取窗口列表：${String(e)}`;
  } finally {
    state.loading = false;
  }
}

/** 只刷新集合状态，不重新枚举窗口（事件触发时用，开销更小） */
async function refreshSnapshot() {
  try {
    const snapshot = await api.getSnapshot();
    state.snapshot = snapshot;
    const pinned = new Set(snapshot.pinned);
    state.windows = state.windows.map((w) => ({
      ...w,
      pinned: pinned.has(w.hwnd),
    }));
  } catch {
    /* 快照刷新失败不打断用户操作 */
  }
}

async function toggle(hwnd: number) {
  if (state.busy.has(hwnd)) return;
  state.busy.add(hwnd);
  try {
    const pinned = await api.togglePin(hwnd);
    state.windows = state.windows.map((w) =>
      w.hwnd === hwnd ? { ...w, pinned } : w,
    );
    // 置顶窗口可能被系统重新排序，取一次快照对齐
    await refreshSnapshot();
  } catch (e) {
    state.error = String(e);
  } finally {
    state.busy.delete(hwnd);
  }
}

async function unpinAll() {
  if (state.unpinning || pinnedCount.value === 0) return;
  state.unpinning = true;
  try {
    const n = await api.unpinAll();
    await refreshSnapshot();
    showToast(`已取消 ${n} 个窗口的置顶`);
  } catch (e) {
    state.error = String(e);
  } finally {
    state.unpinning = false;
  }
}

async function toggleAutostart(enabled: boolean) {
  try {
    const actual = await api.setAutostart(enabled);
    if (state.snapshot) state.snapshot.autostart = actual;
    showToast(actual ? "已设置开机自启" : "已取消开机自启");
  } catch (e) {
    state.error = String(e);
  }
}

/** 挂载时调用：先订阅后端推送，再取初始状态。
 *
 * 顺序很重要 —— 热键线程注册完会立刻推一次 pin://changed 用来上报
 * 「热键是否生效」。如果先取快照再订阅，这一条就会丢，界面会显示错误的
 * 热键状态，直到下一次轮询才自愈。 */
async function init() {
  await api.onPinChanged(() => {
    void refreshSnapshot();
  });

  await api.onPinError((p) => {
    state.error = p.message;
  });

  await refresh();
}

export function useAppStore() {
  return {
    state: readonly(state),
    filtered,
    pinnedWindows,
    looseWindows,
    pinnedCount,
    visibleCount,
    hotkeyKeys,
    refresh,
    refreshSnapshot,
    toggle,
    unpinAll,
    toggleAutostart,
    dismissError,
    showToast,
    init,
    setQuery: (q: string) => {
      state.query = q;
    },
    setFilter: (f: FilterMode) => {
      state.filter = f;
    },
    setView: (v: ViewName) => {
      state.view = v;
    },
  };
}
