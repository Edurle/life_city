//! 时间系统：固定时间步长，1 拍 = 1 模拟分钟。与渲染完全解耦。

pub type Tick = u64;

pub const TICKS_PER_HOUR: Tick = 60;
pub const TICKS_PER_DAY: Tick = 1440;

/// 从 0 起算的模拟时钟。
pub fn day_of(tick: Tick) -> u64 {
    tick / TICKS_PER_DAY
}

pub fn hour_of(tick: Tick) -> u64 {
    (tick % TICKS_PER_DAY) / TICKS_PER_HOUR
}

pub fn minute_of(tick: Tick) -> u64 {
    tick % TICKS_PER_HOUR
}

/// 一天分四段：0 凌晨 / 1 上午 / 2 下午 / 3 夜晚。记忆键的特征之一。
pub fn quarter_of_day(tick: Tick) -> u8 {
    (hour_of(tick) / 6) as u8
}

/// 小时是否在 from..to 窗口内（含头不含尾；from > to 表示跨零点，如 22→6）。
pub fn in_hour_window(hour_now: u32, from: u32, to: u32) -> bool {
    if from <= to {
        hour_now >= from && hour_now < to
    } else {
        hour_now >= from || hour_now < to
    }
}

/// 从 tick 起下一次进入 from 小时窗口的拍（时间窗事件不在窗内时跳到这里再触发）。
pub fn next_window_tick(tick: Tick, from: u32) -> Tick {
    let day_start = tick - tick % TICKS_PER_DAY;
    let target = day_start + from as Tick * TICKS_PER_HOUR;
    if target > tick {
        target
    } else {
        target + TICKS_PER_DAY
    }
}

pub fn is_hour_boundary(tick: Tick) -> bool {
    tick.is_multiple_of(TICKS_PER_HOUR)
}

pub fn is_day_boundary(tick: Tick) -> bool {
    tick.is_multiple_of(TICKS_PER_DAY) && tick > 0
}

/// 人类可读时间，用于日志与查看器。
pub fn format_clock(tick: Tick) -> String {
    format!(
        "第{}天 {:02}:{:02}",
        day_of(tick) + 1,
        hour_of(tick),
        minute_of(tick)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_basics() {
        assert_eq!(hour_of(90), 1);
        assert_eq!(minute_of(90), 30);
        assert_eq!(day_of(1500), 1);
        assert_eq!(quarter_of_day(7 * 60), 1);
        assert!(is_hour_boundary(120));
        assert!(!is_hour_boundary(121));
    }

    #[test]
    fn hour_window_supports_wrap() {
        assert!(in_hour_window(23, 22, 6), "跨零点窗口含深夜");
        assert!(in_hour_window(3, 22, 6), "跨零点窗口含凌晨");
        assert!(!in_hour_window(12, 22, 6), "白天不在夜间窗口");
        assert!(in_hour_window(9, 6, 21), "普通窗口");
        assert!(!in_hour_window(21, 6, 21), "含头不含尾");
    }

    #[test]
    fn next_window_tick_skips_to_start() {
        // 第 1 天 20:00 → 下一个 22:00 是同一天。
        assert_eq!(next_window_tick(1440 + 20 * 60, 22), 1440 + 22 * 60);
        // 第 1 天 23:00 → 下一个 22:00 是次日。
        assert_eq!(next_window_tick(1440 + 23 * 60, 22), 2880 + 22 * 60);
    }
}
