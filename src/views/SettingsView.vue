<script setup lang="ts">
import { computed } from "vue";

import * as api from "../api";
import KeyCap from "../components/KeyCap.vue";
import { useAppStore } from "../stores/app";

const { state: s, hotkeyKeys, toggleAutostart, showToast } = useAppStore();

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
