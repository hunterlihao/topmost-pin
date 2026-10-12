//! 启动动画 —— **原生 GDI 窗口**。
//!
//! ★ 为什么不能用 WebView 画启动画面（第一版就是那么做的，被用户指出白屏）：
//!
//! WebView2 的初始化要 1～1.5 秒，而窗口一显示就是白的 —— 用户看到的是
//! 「先白屏，然后才出动画」。启动画面存在的意义**恰恰是填住这段时间**，
//! 用 WebView 去画它，等于把要填的洞变成了白屏本身。
//!
//! 所以这里用原生 Win32 + GDI：
//!   · 在 Tauri 还没开始构建时就创建窗口（进程启动后几十毫秒，而不是 1.5 秒后）
//!   · 自己画每一帧，第一帧就是动画的第一帧，没有「白底加载中」
//!   · 跑在**独立线程**的消息循环上，与 tao 的循环完全隔离，互不干扰
//!
//! 另一个细节：GDI 没有抗锯齿，斜边会有明显锯齿。所以图钉先按 4 倍分辨率
//! 画进临时位图，再用 `HALFTONE` 缩放贴回 —— 只有图钉需要这么处理，
//! 线条和文字都是轴对齐或由 GDI 自己抗锯齿的。
//!
//! 构图与主界面同一套语言：图钉落下、钉穿那条**水面线**（主界面里分隔
//! 「已置顶」与其余窗口的同一条线），随后浮现应用名与进度线。

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW,
    CreatePen, CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect,
    InvalidateRect, Polygon, SelectObject, SetBkMode, SetBrushOrgEx, SetStretchBltMode,
    SetTextColor, StretchBlt, UpdateWindow, DT_CENTER, DT_SINGLELINE, DT_VCENTER, HDC,
    PAINTSTRUCT, HALFTONE, PS_SOLID, SRCCOPY, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    GetDpiForSystem, SetThreadDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
    GetMessageW, GetSystemMetrics, KillTimer, LoadCursorW, PostMessageW, PostQuitMessage,
    RegisterClassExW, SetTimer, ShowWindow, TranslateMessage, UnregisterClassW, MSG,
    WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_PAINT, WM_TIMER, WNDCLASSEXW,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::winapi::to_wide;

pub const MAIN_LABEL: &str = "main";

/// 96 DPI 下的逻辑尺寸
const LOGICAL_W: f32 = 320.0;
const LOGICAL_H: f32 = 196.0;

/* ── 配色：与 tokens.css 完全一致的取值（COLORREF 是 0x00BBGGRR）── */
const PAPER: u32 = 0x00FF_FFFF; // --paper
const INK: u32 = 0x000A_0A0A; // --ink
const RULE: u32 = 0x00E3_E3E3; // --rule

/* ── 时间线（毫秒）── */
const FRAME_MS: u32 = 16;
const DROP_MS: f32 = 380.0; // 图钉下落
const THUD_START: f32 = 360.0; // 落地
const THUD_MS: f32 = 240.0;
const TEXT_START: f32 = 560.0; // 应用名浮现
const TEXT_MS: f32 = 320.0;
const ANIM_MS: f32 = 1120.0; // 进度线填满

/// 动画播完后最多再等主界面多久。超时也必须收尾，否则用户会一直看着动画。
pub const HOLD_MAX_MS: u64 = 6000;

/// 图钉包围盒宽度占窗口宽度的比例（40/320）
const PIN_W_RATIO: f32 = 0.122;
/// 图钉落地后包围盒顶边的位置（相对窗口高度）
const PIN_TOP_RATIO: f32 = 0.194;
/// 水面线的纵向位置（相对窗口高度）
const LINE_Y_RATIO: f32 = 0.347;

const WINDOW_CLASS: &str = "TopMostPinSplash";
const TIMER_ID: usize = 1;
const SS: i32 = 4; // 图钉超采样倍数

/// 图钉轮廓（单位坐标 0..1，与图标、界面图钉同一个形状）
const PIN_SHAPE: &[(f32, f32)] = &[
    (0.263, 0.075),
    (0.737, 0.075),
    (0.737, 0.300),
    (0.688, 0.300),
    (0.570, 0.620),
    (0.527, 0.620),
    (0.527, 0.855),
    (0.500, 0.950),
    (0.473, 0.855),
    (0.473, 0.620),
    (0.430, 0.620),
    (0.312, 0.300),
    (0.263, 0.300),
];

#[derive(Default)]
struct Shared {
    hwnd: AtomicIsize,
    /// 主界面已经画好了，动画可以收尾
    main_ready: AtomicBool,
    /// 窗口已经销毁，避免重复收尾
    closed: AtomicBool,
}

fn shared() -> &'static Shared {
    static S: OnceLock<Shared> = OnceLock::new();
    S.get_or_init(Shared::default)
}

fn start_instant() -> &'static Instant {
    static T: OnceLock<Instant> = OnceLock::new();
    T.get_or_init(Instant::now)
}

fn elapsed_ms() -> f32 {
    start_instant().elapsed().as_millis() as f32
}

/* ── 对外接口 ─────────────────────────────────────────────────────────── */

/// 弹出启动动画。
///
/// **必须尽早调用**（在 `tauri::Builder` 之前）—— 这正是它存在的意义。
/// 立即返回，窗口与动画在独立线程上跑。
pub fn start() {
    std::thread::Builder::new()
        .name("topmost-pin-splash".to_string())
        .spawn(|| unsafe { run() })
        .ok();
}

/// 主界面已经画好了：收掉启动动画。
///
/// 可从任意线程调用 —— 窗口属于它自己的线程，所以这里只投一个 `WM_CLOSE` 过去。
pub fn destroy() {
    let hwnd = shared().hwnd.load(Ordering::SeqCst);
    if hwnd != 0 {
        unsafe {
            PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0);
        }
    }
}

/// 动画是否已经播完（主界面若已就绪就可以收尾了）
pub fn animation_done() -> bool {
    elapsed_ms() >= ANIM_MS
}

/* ── 内部 ─────────────────────────────────────────────────────────────── */

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    match msg {
        // 全部由 WM_PAINT 双缓冲整帧重绘，不需要系统擦背景 —— 否则会闪
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_TIMER => {
            on_timer(hwnd);
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            shared().closed.store(true, Ordering::SeqCst);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

unsafe fn on_timer(hwnd: HWND) {
    let t = elapsed_ms();

    // 动画播完就停掉重绘（省电），只等主界面就绪或超时收尾
    if t >= ANIM_MS {
        KillTimer(hwnd, TIMER_ID);
    }
    if t >= ANIM_MS
        && (shared().main_ready.load(Ordering::SeqCst) || t >= HOLD_MAX_MS as f32)
    {
        // 交给 WM_CLOSE 走正常的销毁路径
        PostMessageW(hwnd, WM_CLOSE, 0, 0);
        return;
    }
    InvalidateRect(hwnd, std::ptr::null(), 0);
}

unsafe fn run() {
    // ★ 本线程显式声明 PerMonitorV2。
    //
    // 进程级的 DPI 感知是 tao 在 Tauri 初始化时才建立的，而启动动画必须跑在那之前 ——
    // 不声明的话 `GetDpiForSystem` 会按「不感知」返回 96，动画窗在高 DPI 屏上会小一圈
    // （本机 150% 缩放，实测拿到 1.00x，窗口只有应为尺寸的 2/3）。
    // 线条是线程级的，正好只影响这个窗口，不干扰 tao 之后对进程的设定。
    SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

    let hinstance = GetModuleHandleW(std::ptr::null());
    let class_name = to_wide(WINDOW_CLASS);

    let mut wc: WNDCLASSEXW = std::mem::zeroed();
    wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
    wc.style = 0x0002 | 0x0001; // CS_HREDRAW | CS_VREDRAW
    wc.lpfnWndProc = Some(wnd_proc);
    wc.hInstance = hinstance;
    wc.hCursor = LoadCursorW(std::ptr::null_mut(), 32512usize as *const u16); // IDC_ARROW
    wc.lpszClassName = class_name.as_ptr();
    RegisterClassExW(&wc);

    // 自己按 DPI 算物理尺寸（这个窗口不经过 tao，没人替我们换算）
    let scale = GetDpiForSystem() as f32 / 96.0;
    let w = (LOGICAL_W * scale).round() as i32;
    let h = (LOGICAL_H * scale).round() as i32;
    let x = (GetSystemMetrics(0) - w) / 2; // SM_CXSCREEN
    let y = (GetSystemMetrics(1) - h) / 2;

    let title = to_wide("窗口置顶助手");
    let hwnd = CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
        class_name.as_ptr(),
        title.as_ptr(),
        WS_POPUP,
        x,
        y,
        w,
        h,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        hinstance,
        std::ptr::null_mut(),
    );
    if hwnd.is_null() {
        crate::log::write("splash", "原生启动动画窗口创建失败");
        return;
    }
    shared().hwnd.store(hwnd as isize, Ordering::SeqCst);

    start_instant();
    // SW_SHOWNA = 显示但不激活，避免把焦点从用户当前窗口抢走
    ShowWindow(hwnd, 8);
    // 立刻同步画第一帧。少了这一步，窗口会先以「未绘制」的状态暴露一瞬 ——
    // 用户看到的正是那一瞬的白底（第一版用 WebView 时白屏更久）。
    UpdateWindow(hwnd);
    SetTimer(hwnd, TIMER_ID, FRAME_MS, None);
    crate::log::write(
        "splash",
        format!("原生启动动画已显示（{w}x{h} @{scale:.2}x），耗时 {:.0}ms", elapsed_ms()),
    );

    let mut msg: MSG = std::mem::zeroed();
    while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    UnregisterClassW(class_name.as_ptr(), hinstance);
    shared().hwnd.store(0, Ordering::SeqCst);
    crate::log::write("splash", "启动动画已关闭");
}

/* ── 绘制 ─────────────────────────────────────────────────────────────── */

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// 加速下落：起点慢、落点快，才有「砸下去」的感觉
fn ease_in(t: f32) -> f32 {
    t * t * t
}

fn lerp_color(a: u32, b: u32, t: f32) -> u32 {
    let f = |shift: u32| -> u32 {
        let ca = (a >> shift) & 0xFF;
        let cb = (b >> shift) & 0xFF;
        (ca as f32 + (cb as f32 - ca as f32) * t).round() as u32
    };
    (f(16) << 16) | (f(8) << 8) | f(0)
}

unsafe fn fill_rect(dc: HDC, x: i32, y: i32, w: i32, h: i32, color: u32) {
    if w <= 0 || h <= 0 {
        return;
    }
    let r = RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };
    let brush = CreateSolidBrush(color);
    FillRect(dc, &r, brush);
    DeleteObject(brush as _);
}

unsafe fn stroke_rect(dc: HDC, w: i32, h: i32, t: i32, color: u32) {
    fill_rect(dc, 0, 0, w, t, color);
    fill_rect(dc, 0, h - t, w, t, color);
    fill_rect(dc, 0, 0, t, h, color);
    fill_rect(dc, w - t, 0, t, h, color);
}

/// 画图钉：先按 4 倍分辨率画进临时位图，再 HALFTONE 缩回，换取抗锯齿
unsafe fn draw_pin(dc: HDC, x: i32, y: i32, size: i32) {
    let pw = size * SS;
    let ph = size * SS;
    let ss = CreateCompatibleDC(dc);
    let bmp = CreateCompatibleBitmap(dc, pw, ph);
    let old_bmp = SelectObject(ss, bmp as _);

    // 背景必须是纸色：下面是无损缩放贴回，透明会露出没初始化的位图
    fill_rect(ss, 0, 0, pw, ph, PAPER);

    let pts: Vec<POINT> = PIN_SHAPE
        .iter()
        .map(|(fx, fy)| POINT {
            x: (fx * pw as f32).round() as i32,
            y: (fy * ph as f32).round() as i32,
        })
        .collect();

    let brush = CreateSolidBrush(INK);
    let pen = CreatePen(PS_SOLID, 1, INK);
    let old_brush = SelectObject(ss, brush as _);
    let old_pen = SelectObject(ss, pen as _);
    Polygon(ss, pts.as_ptr(), pts.len() as i32);
    SelectObject(ss, old_brush);
    SelectObject(ss, old_pen);
    DeleteObject(brush as _);
    DeleteObject(pen as _);

    SetStretchBltMode(dc, HALFTONE);
    SetBrushOrgEx(dc, 0, 0, std::ptr::null_mut());
    StretchBlt(dc, x, y, size, size, ss, 0, 0, pw, ph, SRCCOPY);

    SelectObject(ss, old_bmp);
    DeleteObject(bmp as _);
    DeleteDC(ss);
}

/// 应用名。单色下没有透明度可用，所以「淡入」做成纸色→墨色的灰度渐变，
/// 这在黑白体系里是合法的值过渡。
unsafe fn draw_wordmark(dc: HDC, w: i32, h: i32, scale: f32, t: f32) {
    let a = clamp01((t - TEXT_START) / TEXT_MS);
    if a <= 0.01 {
        return;
    }
    let color = lerp_color(PAPER, INK, a);

    let font_h = (13.5 * scale).round() as i32;
    let face = to_wide("Microsoft YaHei UI");
    let font = CreateFontW(
        -font_h,
        0,
        0,
        0,
        600, // FW_SEMIBOLD
        0,
        0,
        0,
        1, // DEFAULT_CHARSET
        4, // OUT_TT_PRECIS
        0, // CLIP_DEFAULT_PRECIS
        5, // CLEARTYPE_QUALITY
        0, // DEFAULT_PITCH | FF_DONTCARE
        face.as_ptr(),
    );
    let old_font = SelectObject(dc, font as _);
    SetTextColor(dc, color);
    SetBkMode(dc, TRANSPARENT as i32);

    let text = to_wide("窗口置顶助手");
    // 上浮 5px：随淡入一起完成
    let rise = ((1.0 - a) * 5.0 * scale).round() as i32;
    let top = (h as f32 * 0.545).round() as i32 - rise;
    let mut r = RECT {
        left: 0,
        top,
        right: w,
        bottom: top + (h as f32 * 0.16).round() as i32,
    };
    DrawTextW(
        dc,
        text.as_ptr(),
        -1,
        &mut r,
        DT_CENTER | DT_SINGLELINE | DT_VCENTER,
    );

    SelectObject(dc, old_font);
    DeleteObject(font as _);
}

unsafe fn paint(hwnd: HWND) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);

    let mut rc: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rc);
    let w = rc.right;
    let h = rc.bottom;
    if w <= 0 || h <= 0 {
        EndPaint(hwnd, &ps);
        return;
    }

    // 整帧画在内存 DC 上再一次性贴出，避免逐元素刷新造成的闪烁
    let mem = CreateCompatibleDC(hdc);
    let bmp = CreateCompatibleBitmap(hdc, w, h);
    let old_bmp = SelectObject(mem, bmp as _);

    let scale = w as f32 / LOGICAL_W;
    let t = elapsed_ms();

    fill_rect(mem, 0, 0, w, h, PAPER);

    /* ── 图钉 ──
     * 先画图钉、再画水面线：两者同为墨色，重叠处看不出接缝，
     * 而轴对齐的线不会有锯齿，正好把「钉穿」的效果交给它。 */
    let pin_size = (w as f32 * PIN_W_RATIO).round() as i32;
    let drop = ease_in(clamp01(t / DROP_MS));
    let rest_y = (h as f32 * PIN_TOP_RATIO).round() as i32;
    let pin_y = rest_y - ((1.0 - drop) * pin_size as f32 * 1.45).round() as i32;
    draw_pin(mem, (w - pin_size) / 2, pin_y, pin_size);

    /* ── 水面线 ── */
    let thud = if t >= THUD_START {
        let p = clamp01((t - THUD_START) / THUD_MS);
        (1.5 * scale * (std::f32::consts::PI * p).sin()).round() as i32
    } else {
        0
    };
    let line_h = (2.0 * scale).round().max(1.0) as i32;
    let line_x = (w as f32 * 0.082).round() as i32;
    let line_y = (h as f32 * LINE_Y_RATIO).round() as i32 + thud;
    fill_rect(mem, line_x, line_y, w - line_x * 2, line_h, INK);

    /* ── 应用名 ── */
    draw_wordmark(mem, w, h, scale, t);

    /* ── 进度线 ── */
    let bar_w = (w as f32 * 0.4625).round() as i32;
    let bar_x = (w - bar_w) / 2;
    let bar_h = (1.0 * scale).round().max(1.0) as i32;
    let bar_y = h - (h as f32 * 0.20).round() as i32;
    fill_rect(mem, bar_x, bar_y, bar_w, bar_h, RULE);
    fill_rect(
        mem,
        bar_x,
        bar_y,
        (bar_w as f32 * clamp01(t / ANIM_MS)).round() as i32,
        bar_h,
        INK,
    );

    /* ── 1px 墨色边框：把自己框成「一个对象」，与界面其它对象同一套几何 ── */
    stroke_rect(mem, w, h, (1.0 * scale).round().max(1.0) as i32, INK);

    BitBlt(hdc, 0, 0, w, h, mem, 0, 0, SRCCOPY);

    SelectObject(mem, old_bmp);
    DeleteObject(bmp as _);
    DeleteDC(mem);
    EndPaint(hwnd, &ps);
}
