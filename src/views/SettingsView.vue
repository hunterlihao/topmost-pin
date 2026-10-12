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
  if (recordHintTimer) {
    window.clearTimeout(recordHintTimer);
    recordHintTimer = undefined;
  }
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

  if (e.key === "Escape") {
    stopRecording();
    return;
  }

  const key = keyNameOf(e);
  if (!key) {
    // 只按了修饰键就继续等主键，不打断录制；按了别的键才值得报错
    if (!e.ctrlKey && !e.altKey && !e.shiftKey) {
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
    <!-- ── 运行权限（只读：程序固定以管理员运行，没有开关）───── -->
    <div class="group">
      <h2 class="label group__title">运行权限</h2>
      <div class="group__col">
        <div class="group__row">
          <span class="tag mono" :class="{ 'tag--warn': !isAdmin }">
            {{ isAdmin ? "已以管理员身份运行" : "当前以普通权限运行" }}
          </span>
        </div>

        <p class="group__note">
          本程序固定以管理员身份运行。置顶受 Windows 完整性级别限制（UIPI）：普通权限进程无法操作「以管理员身份运行」的窗口 —— 而浏览器、IDE、资源管理器往往正是这一类，所以这一项不做成可关闭的开关。
        </p>

        <!-- 中文换行会被渲染成一个空格，所以整句写在一行里，不要中途断行 -->
        <p v-if="!isAdmin" class="notice">
          本次提权未成功（UAC 被拒绝，或当前账户没有管理员权限），因此无法置顶提权的窗口。重新启动程序，并在 UAC 提示中选「是」即可。
        </p>
      </div>
    </div>

    <!-- ── 热键 ─────────────────────────────────────────── -->
    <div class="group">
      <h2 class="label group__title">热键</h2>
      <div class="group__col">
        <!-- 录制中：接管键盘，按下组合键即生效 -->
        <div v-if="recording" class="group__row">
          <span class="recorder mono">请按下新的组合键…</span>
          <button class="btn btn-sm" type="button" @click="stopRecording">
            取消
          </button>
        </div>

        <div v-else class="group__row">
          <KeyCap :keys="hotkeyKeys" :off="!s.snapshot?.hotkeyActive" />
          <button class="btn btn-sm" type="button" @click="startRecording">
            修改
          </button>
          <span class="group__note">
            {{
              s.snapshot?.hotkeyActive
                ? "指向任意窗口按下即可置顶，再按一次取消。"
                : "当前没有可用的热键，界面仍可正常置顶。"
            }}
          </span>
        </div>

        <p v-if="hotkeyError" class="notice">{{ hotkeyError }}</p>
        <p v-else-if="recording" class="group__note">
          必须带至少一个修饰键（Ctrl / Alt / Shift），主键支持字母、数字与 F1–F24；按 Esc 取消。
        </p>
      </div>
    </div>

    <!-- ── 开机自启 ─────────────────────────────────────── -->
    <div class="group">
      <h2 class="label group__title">开机自启</h2>
      <div class="group__col">
        <div class="group__row">
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
          <span class="group__note">登录后自动在后台运行。</span>
        </div>

        <!-- 自启与提权是个真实的坑：注册表启动项无法自动提权 -->
        <p v-if="autostart" class="notice notice--quiet">
          注册表启动项无法自动提权，所以每次登录都会弹一次 UAC；选择「是」之后程序才以管理员身份重新启动。
        </p>
      </div>
    </div>

    <!-- ── 数据 ─────────────────────────────────────────── -->
    <div class="group">
      <h2 class="label group__title">数据</h2>
      <div class="group__col">
        <p class="path mono selectable">{{ s.snapshot?.dbPath ?? "—" }}</p>
        <p class="group__note">
          SQLite 单文件，存设置项与置顶事件历史（已记录
          <strong class="mono">{{ s.snapshot?.pinEventCount ?? 0 }}</strong>
          条）。
          {{
            s.snapshot?.portable
              ? "位于程序同目录，整个文件夹拷走即可，系统里不留痕迹。"
              : "程序目录不可写，已回退到系统目录。把程序放到可写目录即可恢复便携。"
          }}
        </p>
        <div class="actions">
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

    <!-- ── 操作 ─────────────────────────────────────────── -->
    <div class="group">
      <h2 class="label group__title">操作</h2>
      <div class="group__row">
        <button class="btn btn-sm" type="button" @click="api.hideToTray()">
          隐藏到托盘
        </button>
        <button class="btn btn-sm" type="button" @click="api.quitApp()">
          退出
        </button>
        <span class="group__note"
          >关掉窗口只是收进托盘，程序仍会保持已置顶的窗口在上层。</span
        >
      </div>
    </div>

    <!-- ── 关于 ─────────────────────────────────────────── -->
    <div class="group">
      <h2 class="label group__title">关于</h2>
      <div class="group__row">
        <span class="mono about__ver">v{{ version }}</span>
        <span class="group__note">
          置顶状态不写入磁盘 —— 窗口句柄重启后即失效。正常退出时程序会自动取消全部置顶；若程序异常结束留下了残留，重新运行后点一次「全部取消」即可清除。
        </span>
      </div>
    </div>
  </section>
</template>

<style scoped>
.view {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}

.group {
  display: grid;
  grid-template-columns: 132px minmax(0, 1fr);
  gap: 16px;
  padding: 18px 20px;
  border-bottom: 1px solid var(--rule);
}
.group__title {
  padding-top: 3px;
}
.group__row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px 14px;
}
.group__col {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}
.group__note {
  color: var(--ink-3);
  font-size: 12.5px;
  line-height: 1.6;
}

.path {
  margin: 0;
  padding: 8px 10px;
  border: 1px solid var(--rule);
  border-radius: var(--radius-object);
  background: var(--well);
  font-size: 12px;
  word-break: break-all;
}

.actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

/* 当前权限的只读标签 */
.tag {
  padding: 2px 8px;
  border: 1px solid var(--rule-strong);
  border-radius: var(--radius-object);
  font-size: 11px;
  color: var(--ink-2);
}
.tag--warn {
  border-color: var(--ink);
  background: var(--ink);
  color: var(--on-ink);
}

/* 录制态：反白块，明确「现在键盘归我」。
   轻微闪烁是给「正在等你按键」的状态感，不是装饰。 */
.recorder {
  padding: 3px 10px;
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

/* 设置已保存但需重启时的提示条 */
.notice {
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 0;
  padding: 8px 10px;
  border: 2px solid var(--ink);
  font-size: 12.5px;
}
/* 只是一条提醒、不需要动作时，退成细边框，避免和上面那条抢注意力 */
.notice--quiet {
  border-width: 1px;
  border-color: var(--rule-strong);
  color: var(--ink-3);
}

.about__ver {
  font-size: 13px;
  font-weight: 600;
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

@media (max-width: 720px) {
  .group {
    grid-template-columns: minmax(0, 1fr);
    gap: 10px;
  }
}
</style>
