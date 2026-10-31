//! 新闻生成（M5，设计稿 6.3）：从日志事件里挑出小镇层面值得播报的，
//! 生成新闻文本。日志是唯一事实源，新闻只是它的另一种读法——
//! 呈现层（CLI / living-play / 查看器）调用，日记与年度总结是后续扩展。

use crate::log::LogEvent;
use crate::time::format_clock;

/// 把一条日志事件转成新闻文本；不值得上报的返回 None。
/// `name_of` / `job_name_of` 由调用方提供（查代理名与职业名）。
pub fn news_text(
    ev: &LogEvent,
    name_of: impl Fn(u32) -> String,
    job_name_of: impl Fn(&str) -> String + Copy,
) -> Option<String> {
    match ev {
        LogEvent::Theft {
            tick,
            thief,
            victim,
            amount,
            caught,
            area,
        } => {
            let when = format_clock(*tick);
            let where_ = area_name(area);
            Some(if *caught {
                format!(
                    "{when}【治安】{}在{where_}行窃时被当场抓获，赃款 {:.0} 已归还失主 {}，将执行社区服务",
                    name_of(*thief),
                    amount,
                    name_of(*victim)
                )
            } else {
                format!(
                    "{when}【治安】{}在{where_}被盗 {:.0}，邻居们议论纷纷",
                    name_of(*victim),
                    amount
                )
            })
        }
        LogEvent::JobChange {
            tick,
            agent,
            from,
            to,
            reason,
        } => {
            let when = format_clock(*tick);
            let who = name_of(*agent);
            let from = from
                .as_deref()
                .map(job_name_of)
                .unwrap_or_else(|| "无业".to_string());
            let to = to
                .as_deref()
                .map(job_name_of)
                .unwrap_or_else(|| "无业".to_string());
            if from == to {
                return None;
            }
            Some(format!("{when}【民生】{who}从{from}变成了{to}（{reason}）"))
        }
        _ => None,
    }
}

/// 区域 ID 的兜底显示（配置里通常有中文名，这里只做兜底）。
fn area_name(id: &str) -> String {
    match id {
        "" => "镇上某处".to_string(),
        other => other.to_string(),
    }
}
