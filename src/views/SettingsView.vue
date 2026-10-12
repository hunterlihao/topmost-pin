<script setup lang="ts">
import { computed, onUnmounted, ref } from "vue";

import * as api from "../api";
import KeyCap from "../components/KeyCap.vue";
import { useAppStore } from "../stores/app";

const { state: s, hotkeyKeys, toggleAutostart, rebindHotkey, showToast } =
  useAppStore();

const version = computed(() => s.snapshot?.version ?? "—");
const autostart = computed(() => s.snapshot?.autostart ?? false);
/**
 * 本进程是否真的提权成功了。
 *
 * 程序固定尝试以管理员运行，所以这里不是「设置值」，而是「这次成没成」——
 * 用户拒绝了 UAC、或账户本身没有管理员权限时会是 false。
 */
const isAdmin = computed(() => s.snapshot?.isAdmin ?? false);

async function onAutostart() {
  await toggleAutostart(!autostart.value);
}

/* ── 热键录制 ────────────────────────────────────────────────────────── */

const recording = ref(false);
const hotkeyError = ref("");
let recordHintTimer: number | undefined;

/** 只按了这些键还不算「按了主键」，录制要继续等 */
const MODIFIER_KEYS = new Set([
  "Control",
  "Alt",
  "Shift",
  "Meta",
  "AltGraph",
  "CapsLock",
]);

function clearHintTimer() {
  if (recordHintTimer) {
    window.clearTimeout(recordHintTimer);
    recordHintTimer = undefined;
  }
}

function startRecording() {
  hotkeyError.value = "";
  recording.value = true;
  // 捕获阶段监听：抢在 WebView 自己的快捷键（Ctrl+R 之类）之前拿到按键
  window.addEventListener("keydown", onRecordKey, true);
  // ★ 已被系统/其它程序注册的组合，会被系统在窗口收到 keydown **之前**截走。
  // 用户按了没反应只会以为坏了，所以过一会儿主动解释一句。
  recordHintTimer = window.setTimeout(() => {
    if (recording.value) {
      hotkeyError.value =
        "还没收到按键。如果这个组合已被其它程序注册，系统会先把它截走、本程序收不到，换一个试试。";
    }
  }, 2500);
}

function stopRecording() {
  recording.value = false;
  window.removeEventListener("keydown", onRecordKey, true);
  clearHintTimer();
}

/** 从键盘事件取出主键名。只按了修饰键、或键位不受支持时返回 null。 */
function keyNameOf(e: KeyboardEvent): string | null {
  const code = e.code;
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  return null;
}

function onRecordKey(e: KeyboardEvent) {
  // 录制的含义就是「这一下按键归我」，所以要吃掉默认行为
  e.preventDefault();
  e.stopPropagation();

  // ★ 收到任何按键就撤掉那条「还没收到按键」的提示定时器。
  // 不撤的话，2.5 秒后它会把刚显示出来的具体原因覆盖成一句不相干的话。
  clearHintTimer();

  if (e.key === "Escape") {
    stopRecording();
    return;
  }

  // 后端明确拒绝 Win 键（系统独占）。前端必须同样报错 ——
  // 否则 Win+Alt+P 会被静默记成 Alt+P，绑上一个和用户按的**不一样**的组合。
  if (e.metaKey) {
    hotkeyError.value = "不支持 Win 键作为修饰键，请用 Ctrl / Alt / Shift";
    return;
  }

  const key = keyNameOf(e);
  if (!key) {
    // 只按修饰键就继续等主键，不打断录制。
    // 按了别的键（方向键、Home、空格……）才值得报错 —— 否则那种输入会被
    // 静默吞掉，表现和「被系统截走」一模一样，用户无从分辨。
    if (!MODIFIER_KEYS.has(e.key)) {
      hotkeyError.value = "这个按键不支持，主键请用字母、数字或 F1–F24";
    }
    return;
  }

  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (parts.length === 0) {
    hotkeyError.value = "热键至少要带一个修饰键（Ctrl / Alt / Shift）";
    return;
  }
  parts.push(key);

  const combo = parts.join("+");
  stopRecording();
  void applyHotkey(combo);
}

async function applyHotkey(combo: string) {
  const reason = await rebindHotkey(combo);
  if (reason) {
    // 失败时后端已经把原热键装回去了，这里只需把原因说清楚
    hotkeyError.value = reason;
    return;
  }
  hotkeyError.value = "";
  showToast(`热键已改为 ${combo}`);
}

onUnmounted(stopRecording);

async function onOpenDir() {
  try {
    await api.openDataDir();
  } catch (e) {
    showToast(String(e));
  }
}

async function onCopy(text: string, what: string) {
  try {
    await navigator.clipboard.writeText(text);
    showToast(`已复制${what}`);
  } catch {
    showToast("复制失败");
  }
}
</script>

<template>
  <section class="view">
    <div class="rows">
      <!-- ── 运行权限（只读：程序固定以管理员运行，没有开关）───────── -->
      <div class="row">
        <div class="row__main">
          <h2 class="row__title">运行权限</h2>
          <p class="row__desc">
            固定以管理员身份运行，不做成开关 —— 普通权限的进程动不了管理员窗口，而浏览器、IDE 往往正是这一类。
          </p>
        </div>
        <div class="row__control">
          <span class="tag" :class="{ 'tag--alert': !isAdmin }">
            {{ isAdmin ? "已以管理员身份运行" : "当前以普通权限运行" }}
          </span>
        </div>
      </div>

      <!-- ── 全局热键 ─────────────────────────────────────────────── -->
      <div class="row">
        <div class="row__main">
          <h2 class="row__title">全局热键</h2>
          <p class="row__desc">
            {{
              s.snapshot?.hotkeyActive
                ? "指向任意窗口按下即可置顶，再按一次取消。在任何程序里都有效。"
                : "当前没有可用的热键 —— 多半是被别的程序占用了。界面仍可正常置顶。"
            }}
          </p>
        </div>
        <div class="row__control">
          <!-- 录制中：接管键盘，按下组合键即生效 -->
          <template v-if="recording">
            <span class="recorder mono">请按下新的组合键…</span>
            <button class="btn btn-sm" type="button" @click="stopRecording">
              取消
            </button>
          </template>
          <template v-else>
            <KeyCap :keys="hotkeyKeys" :off="!s.snapshot?.hotkeyActive" />
            <button class="btn btn-sm" type="button" @click="startRecording">
              修改
            </button>
          </template>
        </div>
        <!-- 录制提示与失败原因横跨整行：它们是要读的句子，挤在按钮旁边读不完 -->
        <p v-if="hotkeyError" class="row__notice">{{ hotkeyError }}</p>
        <p v-else-if="recording" class="row__notice row__notice--quiet">
          必须带至少一个修饰键（Ctrl / Alt / Shift），主键支持字母、数字与 F1–F24；按 Esc 取消。
        </p>
      </div>

      <!-- ── 开机自启 ─────────────────────────────────────────────── -->
      <div class="row">
        <div class="row__main">
          <h2 class="row__title">开机自启</h2>
          <p class="row__desc">登录后自动在后台运行，不弹出窗口。</p>
        </div>
        <div class="row__control">
          <button
            class="switch"
            type="button"
            role="switch"
            :aria-checked="autostart"
            @click="onAutostart"
          >
            <span
              class="switch__track"
              :class="{ 'switch__track--on': autostart }"
            >
              <span class="switch__knob"></span>
            </span>
            <span class="switch__text">{{ autostart ? "已开启" : "已关闭" }}</span>
          </button>
        </div>
        <!-- 自启与提权是个真实的坑：注册表启动项无法自动提权。
             和上面的录制提示一样横跨整行，绝不塞进说明列里。 -->
        <p v-if="autostart" class="row__notice row__notice--quiet">
          注册表启动项无法自动提权，所以每次登录都会弹一次 UAC；选择「是」之后程序才以管理员身份重新启动。
        </p>
      </div>

      <!-- ── 数据与状态 ───────────────────────────────────────────── -->
      <div class="row">
        <div class="row__main">
          <h2 class="row__title">数据与状态</h2>
          <p class="row__desc">
            设置项与置顶事件存在一个 SQLite 单文件里。置顶状态不写入磁盘 —— 绑定窗口句柄，重启即失效，正常退出会自动取消全部置顶；异常退出的残留，重新运行后点一次「全部取消」即可清除。
          </p>
          <p class="row__meta mono">
            已记录 {{ s.snapshot?.pinEventCount ?? 0 }} 条置顶事件 ·
            {{
              s.snapshot?.portable
                ? "便携模式，数据在程序目录"
                : "程序目录不可写，数据已回退到系统目录"
            }}
          </p>
          <p class="path mono selectable">{{ s.snapshot?.dbPath ?? "—" }}</p>
        </div>
        <div class="row__control">
          <button class="btn btn-sm" type="button" @click="onOpenDir">
            打开数据目录
          </button>
          <button
            class="btn btn-sm"
            type="button"
            @click="onCopy(s.snapshot?.dbPath ?? '', '数据库路径')"
          >
            复制数据库路径
          </button>
        </div>
      </div>
    </div>

    <!-- ── 页脚：应用级动作，不是设置项 ───────────────────────────── -->
    <footer class="foot">
      <button class="btn btn-sm" type="button" @click="api.hideToTray()">
        隐藏到托盘
      </button>
      <button class="btn btn-sm" type="button" @click="api.quitApp()">
        退出
      </button>
      <span class="foot__note">
        关掉窗口只是收进托盘，程序仍会保持已置顶的窗口在上层。
      </span>
      <span class="foot__ver mono">v{{ version }}</span>
    </footer>
  </section>
</template>

<style scoped>
.view {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}

.rows {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}

/* 设置行：左边是「这是什么 + 为什么」，右边是「怎么改」。
   标签列 + 内容列的旧布局在 940px 宽度下给说明文字留了一大块右半空白，
   而行是横向的 —— 说明读起来要跨过整行，控件却在下一行。 */
.row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: center;
  gap: 4px 24px;
  padding: 16px;
  border-bottom: 1px solid var(--rule);
}

/* 最后一行不画分隔线：页脚自己有一条 border-top，
   两条挨在一起会变成一道 2px 的粗线。 */
.rows > .row:last-child {
  border-bottom: 0;
}

.row__main {
  min-width: 0;
}

.row__title {
  font-size: 13px;
  font-weight: 600;
}
.row__desc {
  margin: 3px 0 0;
  max-width: 66ch;
  color: var(--ink-3);
  font-size: 12.5px;
  line-height: 1.65;
}

.row__control {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  flex-wrap: wrap;
  gap: 8px;
}

/* 横跨整行的提示（录制中的说明、失败原因、自启的 UAC 提醒） */
.row__notice {
  grid-column: 1 / -1;
  display: flex;
  align-items: center;
  margin: 8px 0 0;
  padding: 7px 10px;
  border: 2px solid var(--ink);
  font-size: 12.5px;
  line-height: 1.6;
}
.row__notice--quiet {
  border-width: 1px;
  border-color: var(--rule-strong);
  color: var(--ink-3);
}

.row__meta {
  margin: 7px 0 0;
  color: var(--ink-3);
  font-size: 11.5px;
}

.path {
  margin: 6px 0 0;
  padding: 7px 10px;
  border: 1px solid var(--rule);
  border-radius: var(--radius-object);
  background: var(--well);
  color: var(--ink-2);
  font-size: 12px;
  word-break: break-all;
}

/* ── 页脚 ─────────────────────────────────────────
   「隐藏到托盘 / 退出 / 版本号」不是设置项，放在最后一块分组里会让它和
   真正要改的东西混在一起。收进页脚，并且在滚动区之外。 */
.foot {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: none;
  padding: 12px 16px;
  border-top: 1px solid var(--rule);
  background: var(--well);
}
.foot__note {
  min-width: 0;
  margin-left: 4px;
  overflow: hidden;
  color: var(--ink-3);
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.foot__ver {
  margin-left: auto;
  color: var(--ink-3);
  font-size: 12px;
}

/* 录制态：反白块，明确「现在键盘归我」。
   轻微闪烁是给「正在等你按键」的状态感，不是装饰。 */
.recorder {
  padding: 4px 10px;
  border: 1px solid var(--ink);
  border-radius: var(--radius-object);
  background: var(--ink);
  color: var(--on-ink);
  font-size: 12px;
  animation: recorder-blink 1.1s steps(2, end) infinite;
}
@keyframes recorder-blink {
  50% {
    opacity: 0.7;
  }
}
@media (prefers-reduced-motion: reduce) {
  .recorder {
    animation: none;
  }
}

/* ── 开关：方形滑块，与全局「对象 2px 圆角」一致，不做胶囊 ── */
.switch {
  display: inline-flex;
  align-items: center;
  gap: 9px;
  padding: 0;
  border: 0;
  background: transparent;
}
.switch__track {
  position: relative;
  width: 38px;
  height: 20px;
  border: 1px solid var(--rule-strong);
  border-radius: var(--radius-object);
  background: var(--paper);
  transition: background var(--dur) var(--ease),
    border-color var(--dur) var(--ease);
}
.switch__track--on {
  background: var(--ink);
  border-color: var(--ink);
}
.switch__knob {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 14px;
  height: 14px;
  border-radius: var(--radius-object);
  background: var(--ink);
  transition: transform var(--dur) var(--ease),
    background var(--dur) var(--ease);
}
.switch__track--on .switch__knob {
  transform: translateX(18px);
  background: var(--on-ink);
}
.switch__text {
  font-size: 13px;
  color: var(--ink-2);
}

/* 窄窗：控件折到说明下面，右对齐改成左对齐，避免又窄又靠右的怪布局 */
@media (max-width: 720px) {
  .row {
    grid-template-columns: minmax(0, 1fr);
  }
  .row__control {
    justify-content: flex-start;
    margin-top: 4px;
  }
  .foot {
    flex-wrap: wrap;
  }
  .foot__note {
    flex-basis: 100%;
    margin-left: 0;
    white-space: normal;
  }
}
</style>
