<script setup lang="ts">
import KeyCap from "./KeyCap.vue";

/**
 * 空台面 —— 全应用唯一一处「大胆」的地方。
 *
 * 界面 99% 的时间处于「一个窗口都还没置顶」这个状态，而原来这里只是一条
 * 和别处没有区别的提示行：整套设计的签名（反相 = 抬升）在用户最常看到的那一屏
 * **根本不存在**。这个组件就是来补这一屏的。
 *
 * 做法：45° 斜线 + 虚线边界。斜线在工程制图与印刷里表示「预留、尚未占用」，
 * 是单色下表达「空位」的老约定 —— 不引入色相，也不和任何状态色冲突
 * （这套设计里本来就没有状态色）。
 *
 * 空状态不是「暂无数据」，而是一句邀请：把热键按**真实尺寸**画在这块空位里，
 * 「这里会放下一个窗口 —— 这样就能让它落下来」。
 */
defineProps<{ keys: string[]; hotkeyActive: boolean }>();
</script>

<template>
  <div class="slot">
    <div class="slot__text">
      <p class="slot__lead">还没有置顶窗口</p>
      <p v-if="hotkeyActive" class="slot__how">
        指向任意窗口按下即可置顶，再按一次取消；也可以点窗口行左侧的图钉。
      </p>
      <p v-else class="slot__how">
        热键被其它程序占用，暂时按不了；点窗口行左侧的图钉置顶，或到「设置」里换一个组合键。
      </p>
    </div>

    <div v-if="hotkeyActive" class="slot__keys">
      <KeyCap :keys="keys" size="lg" />
    </div>
  </div>
</template>

<style scoped>
.slot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
  flex: none;
  padding: 14px 16px;
  /* 45° 斜线：预留位的纹理。幅度压到 7% 墨，文字压在上面仍然清楚。 */
  background-image: repeating-linear-gradient(
    45deg,
    transparent 0 6px,
    var(--hatch) 6px 7px
  );
  /* 上方是报头的实线（结构边界），下方这条是「预留边界」——
     虚线比实线弱一档，所以用 ink-4 而不是 rule。 */
  border-bottom: 1px dashed var(--ink-4);
}

.slot__text {
  min-width: 0;
}
.slot__lead {
  margin: 0 0 3px;
  font-size: 13.5px;
  font-weight: 600;
}
.slot__how {
  margin: 0;
  max-width: 46ch;
  color: var(--ink-2);
  font-size: 12.5px;
  line-height: 1.6;
}

.slot__keys {
  flex: none;
  padding-right: 4px;
}

@media (max-width: 800px) {
  .slot {
    flex-wrap: wrap;
    gap: 10px;
  }
}
</style>
