<script setup lang="ts">
import { onUnmounted, ref } from "vue";

import * as api from "../api";

/**
 * 权限状态带 —— 只在**没拿到管理员权限**时出现，且只出现在窗口页。
 *
 * 为什么要把它从设置页里搬出来：置顶受 Windows 的 UIPI 限制，普通权限进程
 * 动不了管理员窗口的 z 序，而浏览器 / IDE / 资源管理器恰恰常年是管理员跑起来的
 * —— 也就是说，提权失败时，用户最想置顶的那批窗口**全部**会失败。
 * 这是这个程序最要紧的一件事，原来却只是设置页里的一个小小的边框标签，
 * 用户得先想到「去设置里看看」才能知道为什么点了没反应。
 *
 * 视觉沿用 ErrorStrip 那套：整个界面没有红色可用，「坏消息」只能靠
 * 2px 墨色边框 + 反相标记块 + 字重来表达。
 */
const busy = ref(false);
const failed = ref("");
let fallbackTimer: number | undefined;

async function retry() {
  busy.value = true;
  failed.value = "";
  try {
    await api.relaunchAsAdmin();
    // 成功的话本进程 600ms 后就退出了，下面这句提示多半来不及被看到。
    // 但它仍然必要：万一退出没有真的发生，界面必须显示「正在重启」，
    // 而不是留一个还能再点的按钮（再点就是又弹一次 UAC）。
    fallbackTimer = window.setTimeout(() => {
      busy.value = false;
      failed.value =
        "重启似乎没有发生。请从托盘退出，再手动打开程序一次。";
    }, 5000);
  } catch (e) {
    // 多数情况是用户取消了 UAC。就地显示，和热键录制失败的处理方式一致 ——
    // 动作自己报的错，就报在动作旁边，不丢进全局错误条。
    failed.value = String(e);
    busy.value = false;
  }
}

onUnmounted(() => {
  if (fallbackTimer) window.clearTimeout(fallbackTimer);
});
</script>

<template>
  <div class="band">
    <span class="band__mark" aria-hidden="true">!</span>

    <div class="band__text">
      <p class="band__lead">未取得管理员权限</p>
      <p class="band__body">
        浏览器、IDE、资源管理器这类窗口无法置顶 —— 普通权限的进程动不了管理员窗口。重启会取消当前的全部置顶。
      </p>
      <p v-if="failed" class="band__fail">{{ failed }}</p>
      <p v-else-if="busy" class="band__fail band__fail--quiet">
        正在以管理员身份重启…
      </p>
    </div>

    <div class="band__action">
      <button
        class="btn btn-primary btn-sm"
        type="button"
        :disabled="busy"
        @click="retry"
      >
        以管理员身份重启
      </button>
    </div>
  </div>
</template>

<style scoped>
.band {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  align-items: stretch;
  flex: none;
  margin: 12px 16px;
  border: 2px solid var(--ink);
  background: var(--paper);
}

.band__mark {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  background: var(--ink);
  color: var(--on-ink);
  font-family: var(--font-data);
  font-size: 15px;
  font-weight: 700;
}

.band__text {
  min-width: 0;
  padding: 9px 12px;
}
.band__lead {
  margin: 0 0 2px;
  font-size: 13px;
  font-weight: 600;
}
.band__body {
  margin: 0;
  color: var(--ink-2);
  font-size: 12.5px;
  line-height: 1.6;
}

.band__fail {
  margin: 6px 0 0;
  font-size: 12.5px;
  font-weight: 600;
}
.band__fail--quiet {
  font-weight: 400;
  color: var(--ink-3);
}

.band__action {
  display: flex;
  align-items: center;
  padding: 0 12px;
}

@media (max-width: 800px) {
  .band {
    grid-template-columns: auto minmax(0, 1fr);
  }
  .band__action {
    grid-column: 2;
    padding: 0 12px 10px;
  }
}
</style>
