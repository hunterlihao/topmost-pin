/** 应用状态。单一 reactive 实例，组件通过 useAppStore() 取用。 */

import { computed, reactive, readonly } from "vue";

import * as api from "../api";
import type { Snapshot, WindowInfo } from "../types";

export type ViewName = "windows" | "settings";

const state = reactive({
  windows: [] as WindowInfo[],
  snapshot: null as Snapshot | null,
  loading: true,
  /* 行级操作中的 HWND，用于禁用按钮并显示忙碌态 */
  busy: new Set<number>(),
  unpinning: false,
  query: "",
  view: "windows" as ViewName,
  error: "" as string,
  toast: "" as string,
});

let toastTimer: number | undefined;

/** 归一化：去掉首尾空白并折叠内部连续空白，便于标题匹配 */
function normalize(s: string) {
  return s.trim().replace(/\s+/g, " ");
}

/**
 * 台面（已置顶）上的行。
 *
 * ★ 刻意**不**受搜索影响 —— 这是这一版改版的一个决定。
 *   搜索解决的是「在一堆窗口里找到想抬起来的那一个」，而台面是**状态读数**：
 *   已经被抬起来的东西，不该因为你在下面找别的东西就从屏幕上消失。
 *   顺带也让台面标题上的计数永远等于它下面显示的行数，不会出现
 *   「写着 3 行、只显示 1 行」这种看起来像 bug 的状态。
 */
const pinnedWindows = computed(() => state.windows.filter((w) => w.pinned));

/** 下层候选区 = 未置顶的窗口，受搜索影响 */
const looseWindows = computed(() => {
  const q = normalize(state.query).toLowerCase();
  return state.windows.filter((w) => {
    if (w.pinned) return false;
    if (!q) return true;
    return (
      w.title.toLowerCase().includes(q) ||
      w.processName.toLowerCase().includes(q) ||
      w.className.toLowerCase().includes(q)
    );
  });
});

/** 搜索框里是否有有效关键词（决定「没有匹配」怎么显示） */
const searchActive = computed(() => normalize(state.query).length > 0);

const pinnedCount = computed(() => pinnedWindows.value.length);
const visibleCount = computed(() => state.windows.length);

const hotkeyKeys = computed(() => {
  // ★ 热键没生效时后端返回的是占位文案「未生效」，它不是按键 ——
  // 直接 split 会渲染出一个写着「未生效」的假键帽，看起来像是一个键。
  // 判据用 hotkeyActive，而不是「字符串非空」。
  if (!state.snapshot?.hotkeyActive) return [];
  const hk = state.snapshot.hotkey ?? "";
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

/**
 * 拉取窗口列表与快照。
 *
 * ★ 默认**不清**错误条：这个函数同时被 5 秒轮询和窗口聚焦刷新调用，
 * 而错误条是「按热键失败」这类操作的唯一出口 —— 用户还没读完就被后台轮询
 * 抹掉，表现就是「闪了一下就没了」。只有用户主动点「刷新」才算明确表示要看新状态。
 */
async function refresh(opts: { clearError?: boolean } = {}) {
  try {
    const [windows, snapshot] = await Promise.all([
      api.listWindows(),
      api.getSnapshot(),
    ]);
    state.windows = windows;
    state.snapshot = snapshot;
    if (opts.clearError) state.error = "";
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

/**
 * 修改全局热键。
 *
 * 成功返回 null，失败返回**给用户看的原因** —— 调用方是设置页，用户人就在
 * 那个输入位置旁边，把原因显示在那里比丢进全局错误条有用得多。
 */
async function rebindHotkey(combo: string): Promise<string | null> {
  try {
    const effective = await api.setHotkey(combo);
    if (state.snapshot) {
      state.snapshot.hotkey = effective;
      state.snapshot.hotkeyActive = true;
    }
    return null;
  } catch (e) {
    return String(e);
  }
}

/** 后端推送的注销函数。组件卸载时必须调用 —— 见 init 的注释 */
let unlisteners: Array<() => void> = [];
/** init 只真正跑一次 */
let initPromise: Promise<void> | null = null;

/** 挂载时调用：先订阅后端推送，再取初始状态。
 *
 * 顺序很重要 —— 热键线程注册完会立刻推一次 pin://changed 用来上报
 * 「热键是否生效」。如果先取快照再订阅，这一条就会丢，界面会显示错误的
 * 热键状态，直到下一次轮询才自愈。 */
async function init() {
  // ★ 幂等。tauri 的 listen 没有同名去重，每次调用都会注册一个**新的**后端监听，
  // 重复 init（组件重挂载、HMR）会让每个事件被处理多次、多打一轮 IPC。
  initPromise ??= (async () => {
    try {
      unlisteners.push(
        await api.onPinChanged(() => {
          void refreshSnapshot();
        }),
      );
      unlisteners.push(
        await api.onPinError((p) => {
          state.error = p.message;
        }),
      );
    } catch (e) {
      // 订阅失败不该让界面停摆：退化成只靠轮询刷新，但要说清楚
      state.error = `无法订阅后端事件，将只靠轮询刷新：${String(e)}`;
    }
    await refresh();
  })();
  return initPromise;
}

/** 注销事件监听并复位。组件卸载时调用。 */
function dispose() {
  for (const off of unlisteners) {
    try {
      off();
    } catch {
      /* 注销失败不影响退出 */
    }
  }
  unlisteners = [];
  initPromise = null;
}

export function useAppStore() {
  return {
    state: readonly(state),
    pinnedWindows,
    looseWindows,
    searchActive,
    pinnedCount,
    visibleCount,
    hotkeyKeys,
    refresh,
    refreshSnapshot,
    toggle,
    unpinAll,
    toggleAutostart,
    rebindHotkey,
    dismissError,
    showToast,
    init,
    dispose,
    setQuery: (q: string) => {
      state.query = q;
    },
    setView: (v: ViewName) => {
      state.view = v;
    },
  };
}
