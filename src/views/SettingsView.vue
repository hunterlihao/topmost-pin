<script setup lang="ts">
import { computed } from "vue";

import * as api from "../api";
import KeyCap from "../components/KeyCap.vue";
import { useAppStore } from "../stores/app";

const {
  state: s,
  hotkeyKeys,
  toggleAutostart,
  toggleRunAsAdmin,
  showToast,
} = useAppStore();

const version = computed(() => s.snapshot?.version ?? "—");
const autostart = computed(() => s.snapshot?.autostart ?? false);
const runAsAdmin = computed(() => s.snapshot?.runAsAdmin ?? false);
const isAdmin = computed(() => s.snapshot?.isAdmin ?? false);

/** 设置与实际权限不一致：需要重启才能落实 */
const permissionPending = computed(() => runAsAdmin.value !== isAdmin.value);

async function onAutostart() {
  await toggleAutostart(!autostart.value);
}

async function onRunAsAdmin() {
  await toggleRunAsAdmin(!runAsAdmin.value);
}

/** 设置已经写好了，只是当前进程权限不对 —— 再叫一次就会重启 */
async function onApplyPermission() {
  await toggleRunAsAdmin(runAsAdmin.value);
}

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
    <!-- ── 运行权限 ─────────────────────────────────────── -->
    <div class="group">
      <h2 class="label group__title">运行权限</h2>
      <div class="group__col">
        <div class="group__row">
          <button
            class="switch"
            type="button"
            role="switch"
            :aria-checked="runAsAdmin"
            @click="onRunAsAdmin"
          >
            <span
              class="switch__track"
              :class="{ 'switch__track--on': runAsAdmin }"
            >
              <span class="switch__knob"></span>
            </span>
            <span class="switch__text">以管理员身份运行</span>
          </button>
          <span class="tag mono" :class="{ 'tag--warn': permissionPending }">
            {{ isAdmin ? "当前：管理员" : "当前：普通权限" }}
          </span>
        </div>

        <!-- 设置与实际不一致时，必须说清楚「为什么还没生效」并给出出路 -->
        <p v-if="permissionPending" class="notice">
          设置已保存，但需要重启才能生效。
          <button class="btn btn-sm" type="button" @click="onApplyPermission">
            立即重启
          </button>
        </p>

        <p class="group__note">
          开启后可以置顶「以管理员身份运行」的程序（如任务管理器）—— 这是 Windows
          的完整性级别限制（UIPI），普通权限进程无法操作提权窗口。代价是每次启动都会弹
          UAC 提示。切换这个开关会重启程序。
        </p>
      </div>
    </div>

    <!-- ── 热键 ─────────────────────────────────────────── -->
    <div class="group">
      <h2 class="label group__title">热键</h2>
      <div class="group__row">
        <KeyCap :keys="hotkeyKeys" :off="!s.snapshot?.hotkeyActive" />
        <span class="group__note">
          {{
            s.snapshot?.hotkeyActive
              ? "指向任意窗口按下即可置顶，再按一次取消。"
              : "候选组合都被其它程序占用，当前未生效。界面仍可正常置顶。"
          }}
        </span>
      </div>
    </div>

    <!-- ── 开机自启 ─────────────────────────────────────── -->
    <div class="group">
      <h2 class="label group__title">开机自启</h2>
      <div class="group__row">
        <button
          class="switch"
          type="button"
          role="switch"
          :aria-checked="autostart"
          @click="onAutostart"
        >
          <span class="switch__track" :class="{ 'switch__track--on': autostart }">
            <span class="switch__knob"></span>
          </span>
          <span class="switch__text">{{ autostart ? "已开启" : "已关闭" }}</span>
        </button>
        <span class="group__note"
          >登录后自动在后台运行，只写当前用户的注册表启动项。</span
        >
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
          置顶状态不写入磁盘 —— 窗口句柄重启后即失效。正常退出时程序会自动取消
          全部置顶；若程序异常结束留下了残留，重新运行后点一次「全部取消」即可清除。
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
