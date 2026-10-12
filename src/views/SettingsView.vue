<script setup lang="ts">
import { computed } from "vue";

import * as api from "../api";
import KeyCap from "../components/KeyCap.vue";
import { useAppStore } from "../stores/app";

const {
  state: s,
  hotkeyKeys,
  toggleAutostart,
  showToast,
} = useAppStore();

const version = computed(() => s.snapshot?.version ?? "—");
const autostart = computed(() => s.snapshot?.autostart ?? false);

async function onAutostart() {
  await toggleAutostart(!autostart.value);
}

async function onHide() {
  await api.hideToTray();
}

async function onQuit() {
  await api.quitApp();
}

function onCopyPath() {
  const dir = s.snapshot?.dataDir;
  if (!dir) return;
  void navigator.clipboard.writeText(dir).then(
    () => showToast("已复制数据目录路径"),
    () => showToast("复制失败"),
  );
}
</script>

<template>
  <section class="view">
    <div class="group">
      <h2 class="label group__title">热键</h2>
      <div class="group__row">
        <KeyCap :keys="hotkeyKeys" :off="!s.snapshot?.hotkeyActive" />
        <span class="group__note">
          {{
            s.snapshot?.hotkeyActive
              ? "指向任意窗口按下即可置顶，再按一次取消。"
              : "被其他程序占用，当前未生效。界面仍可正常置顶。"
          }}
        </span>
      </div>
    </div>

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

    <div class="group">
      <h2 class="label group__title">数据目录</h2>
      <div class="group__col">
        <p class="path mono selectable">{{ s.snapshot?.dataDir ?? "—" }}</p>
        <p class="group__note">
          {{
            s.snapshot?.portable
              ? "位于程序同目录，整个文件夹拷走即可，系统里不留痕迹。"
              : "程序目录不可写，已回退到系统目录。把程序放到可写目录即可恢复便携。"
          }}
        </p>
        <div class="actions">
          <button class="btn btn-sm" type="button" @click="onCopyPath">
            复制路径
          </button>
        </div>
      </div>
    </div>

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
    <div class="group">
      <h2 class="label group__title">操作</h2>
      <div class="group__row">
        <button class="btn btn-sm" type="button" @click="onHide">
          隐藏到托盘
        </button>
        <button class="btn btn-sm" type="button" @click="onQuit">退出</button>
        <span class="group__note"
          >关掉窗口只是收进托盘，程序仍会保持已置顶的窗口在上层。</span
        >
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
