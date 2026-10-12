<script setup lang="ts">
/**
 * 启动动画。
 *
 * 动作选的是「图钉落下、钉穿那条水面线」—— 与主界面同一套语言：
 * 置顶即抬升，而抬升必须有东西被钉住。整段只用一个意象，不做多余装饰。
 *
 * 时长归前端管（要尊重 prefers-reduced-motion），Rust 侧只做 5 秒兜底。
 */
import { onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";

/** 正常时长。与下面动画的总时长保持一致。 */
const FULL_MS = 1120;
/** 用户要求减少动效时，只留一个可感知的停顿 */
const REDUCED_MS = 380;

const PIN_PATH =
  "M16 9V4h1c.55 0 1-.45 1-1s-.45-1-1-1H7c-.55 0-1 .45-1 1s.45 1 1 1h1v5c0 1.66-1.34 3-3 3v2h5.97v7l1 1 1-1v-7H19v-2c-1.66 0-3-1.34-3-3z";

onMounted(() => {
  const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (reduced) document.documentElement.dataset.reduced = "1";

  window.setTimeout(
    () => {
      void invoke("finish_splash");
    },
    reduced ? REDUCED_MS : FULL_MS,
  );
});
</script>

<template>
  <div class="splash">
    <div class="stage">
      <!-- 水面线：主界面里分隔「已置顶」与其余窗口的那条线，这里是同一个符号 -->
      <div class="surface"></div>
      <div class="pin">
        <svg viewBox="0 0 24 24" width="34" height="34" aria-hidden="true">
          <path :d="PIN_PATH" fill="currentColor" />
        </svg>
      </div>
    </div>

    <p class="wordmark">窗口置顶助手</p>
    <div class="progress"><span class="progress__fill"></span></div>
  </div>
</template>

<style scoped>
:global(html),
:global(body),
:global(#app) {
  height: 100%;
  margin: 0;
}

:global(body) {
  background: var(--paper);
  color: var(--ink);
  font-family: var(--font-ui);
  overflow: hidden;
  user-select: none;
  cursor: default;
}

.splash {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  height: 100%;
  box-sizing: border-box;
  /* 无边框窗口：用 1px 墨线把自己框成「一个对象」，与界面上其它对象同一套几何 */
  border: 1px solid var(--ink);
  background: var(--paper);
}

/* ── 图钉与水面线 ─────────────────────────────────────── */

.stage {
  position: relative;
  width: 100%;
  height: 96px;
  animation: thud 220ms ease-out 360ms both;
}

.surface {
  position: absolute;
  left: 26px;
  right: 26px;
  top: 68px;
  height: 2px;
  background: var(--ink);
  animation: flex 240ms ease-out 360ms both;
}

.pin {
  position: absolute;
  left: 50%;
  top: 38px;
  color: var(--ink);
  transform: translateX(-50%) translateY(-48px);
  animation: drop 380ms cubic-bezier(0.5, 0, 0.9, 0.4) both;
}

@keyframes drop {
  to {
    transform: translateX(-50%) translateY(0);
  }
}
/* 落地瞬间整块轻微下沉一下 —— 让「钉住」这件事有重量 */
@keyframes thud {
  0%,
  100% {
    transform: translateY(0);
  }
  45% {
    transform: translateY(1.5px);
  }
}
@keyframes flex {
  0%,
  100% {
    transform: translateY(0);
  }
  45% {
    transform: translateY(1.5px);
  }
}

/* ── 文字与进度 ───────────────────────────────────────── */

.wordmark {
  margin: 0;
  font-size: 13.5px;
  font-weight: 600;
  letter-spacing: 0.06em;
  animation: rise 320ms var(--ease) 560ms both;
}

@keyframes rise {
  from {
    opacity: 0;
    transform: translateY(5px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

.progress {
  width: 148px;
  height: 1px;
  background: var(--rule);
  overflow: hidden;
}

.progress__fill {
  display: block;
  width: 0;
  height: 100%;
  background: var(--ink);
  animation: fill 1060ms linear both;
}

@keyframes fill {
  to {
    width: 100%;
  }
}

/* ── 减少动效 ─────────────────────────────────────────── */

:global(html[data-reduced="1"]) .stage,
:global(html[data-reduced="1"]) .surface,
:global(html[data-reduced="1"]) .pin,
:global(html[data-reduced="1"]) .wordmark,
:global(html[data-reduced="1"]) .progress__fill {
  animation: none;
}

:global(html[data-reduced="1"]) .pin {
  transform: translateX(-50%) translateY(0);
}

:global(html[data-reduced="1"]) .wordmark {
  opacity: 1;
}

:global(html[data-reduced="1"]) .progress__fill {
  width: 100%;
}
</style>
