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
}
