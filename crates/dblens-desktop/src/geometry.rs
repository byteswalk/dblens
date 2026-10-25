//! 窗口位置和大小的计算（设计文档 7.2 节），不依赖 Tauri，单独测试。坐标和尺寸都是物理像素。

use serde::{Deserialize, Serialize};

/// 默认大小与最小大小（逻辑像素）。
pub const DEFAULT_SIZE: (f64, f64) = (1440.0, 900.0);
pub const MIN_SIZE: (f64, f64) = (1024.0, 640.0);

/// 窗口顶部这么高的条带算作标题栏。
const TITLE_BAR: i64 = 40;
/// 标题栏至少要有这么大的一块落在某个显示器的工作区内，用户才拖得动窗口。
const REACHABLE: (i64, i64) = (100, 20);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// 默认位置和大小：在 `work_area`（显示器去掉任务栏后的区域）中居中，默认 1440×900；
/// 某个方向上放不下时，这个方向取工作区的 90%。`scale` 是显示器的缩放比例。
pub fn default_rect(work_area: Rect, scale: f64) -> Rect {
    let fit = |logical: f64, available: u32| {
        let want = (logical * scale).round();
        if want <= f64::from(available) { want as u32 } else { (f64::from(available) * 0.9).round() as u32 }
    };
    let width = fit(DEFAULT_SIZE.0, work_area.width);
    let height = fit(DEFAULT_SIZE.1, work_area.height);
    Rect {
        x: work_area.x + ((work_area.width - width) / 2) as i32,
        y: work_area.y + ((work_area.height - height) / 2) as i32,
        width,
        height,
    }
}

/// 保存的窗口位置是否还能用：标题栏有足够大的一块落在某个显示器的工作区内。
/// 显示器被拔掉或分辨率变小后，返回 false，由调用方改用主屏幕上的默认位置。
pub fn is_reachable(rect: Rect, work_areas: &[Rect]) -> bool {
    let (left, top) = (i64::from(rect.x), i64::from(rect.y));
    let (right, bottom) = (left + i64::from(rect.width), top + TITLE_BAR.min(i64::from(rect.height)));
    work_areas.iter().any(|a| {
        let (al, at) = (i64::from(a.x), i64::from(a.y));
        let (ar, ab) = (al + i64::from(a.width), at + i64::from(a.height));
        let w = right.min(ar) - left.max(al);
        let h = bottom.min(ab) - top.max(at);
        w >= REACHABLE.0 && h >= REACHABLE.1
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: i32, y: i32, width: u32, height: u32) -> Rect {
        Rect { x, y, width, height }
    }

    #[test]
    fn default_is_centered_1440x900() {
        // 1920×1080 的屏幕，任务栏 40 像素
        assert_eq!(default_rect(r(0, 0, 1920, 1040), 1.0), r(240, 70, 1440, 900));
        // 副屏在主屏右边
        assert_eq!(default_rect(r(1920, 0, 2560, 1400), 1.0), r(1920 + 560, 250, 1440, 900));
    }

    #[test]
    fn default_follows_display_scale() {
        // 2560×1600、150% 缩放：1440×900 逻辑像素 = 2160×1350 物理像素
        assert_eq!(default_rect(r(0, 0, 2560, 1552), 1.5), r(200, 101, 2160, 1350));
    }

    #[test]
    fn small_screen_uses_90_percent_of_work_area() {
        // 1366×768 的屏幕：两个方向都放不下
        assert_eq!(default_rect(r(0, 0, 1366, 728), 1.0), r(68, 36, 1229, 655));
        // 1600×900：宽度放得下，高度放不下
        assert_eq!(default_rect(r(0, 0, 1600, 860), 1.0), r(80, 43, 1440, 774));
    }

    #[test]
    fn reachable_only_when_title_bar_is_on_some_monitor() {
        let monitors = [r(0, 0, 1920, 1040), r(1920, 0, 1920, 1040)];
        assert!(is_reachable(r(100, 100, 1440, 900), &monitors));
        assert!(is_reachable(r(2000, 100, 1440, 900), &monitors), "在副屏上");
        assert!(is_reachable(r(-1340, 100, 1440, 900), &monitors), "左边露出 100 像素");
        assert!(!is_reachable(r(-1400, 100, 1440, 900), &monitors), "只露出 40 像素");
        assert!(!is_reachable(r(4000, 100, 1440, 900), &monitors), "副屏已拔掉");
        assert!(!is_reachable(r(100, -500, 1440, 900), &monitors), "标题栏在屏幕上方");
        assert!(!is_reachable(r(100, 1030, 1440, 900), &monitors), "标题栏只露出 10 像素");
        assert!(!is_reachable(r(100, 100, 1440, 900), &[]));
    }
}
