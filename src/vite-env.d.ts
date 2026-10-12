/// <reference types="vite/client" />

// 刻意**不**声明 `declare module "*.vue"`：
// 那条 shim 会把所有 .vue 导入退化成 DefineComponent<{}, {}, any>，
// 于是组件的 props/emits 全变成 any —— vue-tsc 就再也查不出传错的 prop 了。
// Volar（vue-tsc）本身就能解析 SFC，不需要这层声明。
