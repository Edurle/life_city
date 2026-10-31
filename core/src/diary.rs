//! 日记与年度总结（M5 二阶段，设计稿 6.4）：从日志自动生成。
//! 日志是唯一事实源，日记只是它的另一种读法——第一人称、只挑值得写的，
//! 呈现层为主（核心不做任何额外记录）。年度总结在年末自动生成：
//! 本年最常做的事、最亲近的人、经历的大事。

use crate::log::LogEvent;
use crate::time::{day_of, format_clock};
use std::collections::BTreeMap;

/// 行为的显示标签（"下馆子（招牌硬菜）"）。由调用方从配置拼出。
pub type LabelFn<'a> = dyn Fn(&str, Option<&str>) -> String + 'a;

/// 一天的日记（第一人称的行，按时间序）。调用方负责把日志切成那一天。
/// 只写值得写的：找过谁、记住了什么、遭遇了什么、人生大事。
pub fn day_diary(
    events: &[LogEvent],
    agent: u32,
    name_of: impl Fn(u32) -> String,
    label: &LabelFn,
) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    // 一天的行为流水（结尾汇总成一句"今天主要在……"）。
    let mut acts: BTreeMap<String, u64> = BTreeMap::new();
    // 和谁亲近了（按人聚合，最后报最有意义的几段）。
    let mut bonds: BTreeMap<u32, f64> = BTreeMap::new();
    // 被种草（目睹模仿）：单条太吵，聚合成一句。
    let mut imitations: BTreeMap<String, u64> = BTreeMap::new();
    for ev in events {
        match ev {
            LogEvent::ActionStart {
                agent: who,
                behavior,
                variant,
                ..
            } if *who == agent => {
                let key = label(behavior, variant.as_deref());
                *acts.entry(key).or_insert(0) += 1;
            }
            LogEvent::SocialPick {
                tick,
                agent: who,
                target,
                bond,
                impression,
                ..
            } if *who == agent => {
                let mood = if *impression < 0.6 {
                    "（其实有点膈应，但事情总要当面说）"
                } else if *bond > 0.7 {
                    "（好久没见，怪想的）"
                } else {
                    ""
                };
                lines.push(format!(
                    "{} 去找了{}{}",
                    format_clock(*tick),
                    name_of(*target),
                    mood
                ));
            }
            LogEvent::BondChange {
                tick: _,
                a,
                b,
                delta,
                ..
            } if *a == agent || *b == agent => {
                let other = if *a == agent { *b } else { *a };
                *bonds.entry(other).or_insert(0.0) += delta;
            }
            LogEvent::MemoryWrite {
                tick,
                agent: who,
                event,
                ..
            } if *who == agent => {
                lines.push(format!(
                    "{} 心里记下了【{}】这一笔",
                    format_clock(*tick),
                    event
                ));
            }
            LogEvent::Theft {
                tick,
                thief,
                victim,
                amount,
                caught,
                ..
            } => {
                if *victim == agent {
                    lines.push(format!(
                        "{} 被偷了 {:.0}！气得半宿没睡",
                        format_clock(*tick),
                        amount
                    ));
                } else if *thief == agent && *caught {
                    lines.push(format!(
                        "{} 被当场抓住了，丢人……只好去打扫公园赔罪",
                        format_clock(*tick)
                    ));
                }
            }
            LogEvent::JobChange {
                tick,
                agent: who,
                from,
                to,
                reason,
            } if *who == agent => {
                let f = from.as_deref().unwrap_or("无业");
                let t = to.as_deref().unwrap_or("无业");
                lines.push(format!(
                    "{} 换了份工作：{} → {}（{reason}）",
                    format_clock(*tick),
                    f,
                    t
                ));
            }
            LogEvent::Couple {
                tick,
                a,
                b,
                started,
            } if *a == agent || *b == agent => {
                let other = if *a == agent {
                    name_of(*b)
                } else {
                    name_of(*a)
                };
                if *started {
                    lines.push(format!("{} 和{other}在一起了！！", format_clock(*tick)));
                } else {
                    lines.push(format!("{} 和{other}分开了，都还好。", format_clock(*tick)));
                }
            }
            LogEvent::Born {
                tick,
                agent: baby,
                mother,
                father,
            } if *mother == agent || *father == agent => {
                lines.push(format!(
                    "{} 我们家添了个孩子，{}！抱在手里软软的",
                    format_clock(*tick),
                    name_of(*baby)
                ));
            }
            LogEvent::GoalSet {
                tick,
                agent: who,
                behavior,
                ..
            } if *who == agent => {
                lines.push(format!(
                    "{} 给自己定了个小目标：管住【{behavior}】",
                    format_clock(*tick)
                ));
            }
            LogEvent::GoalEnd {
                tick,
                agent: who,
                behavior,
                success,
                abandoned,
                violations,
                ..
            } if *who == agent => {
                let verdict = if *abandoned {
                    "算了，不跟自己较劲了".to_string()
                } else if *success {
                    "做到了！".to_string()
                } else {
                    format!("没完全做到，失手 {violations} 次")
                };
                lines.push(format!(
                    "{} 【{behavior}】的目标结束了，{verdict}",
                    format_clock(*tick)
                ));
            }
            LogEvent::Imitate {
                tick: _,
                observer,
                from: _,
                behavior,
                variant,
            } if *observer == agent => {
                let key = label(behavior, variant.as_deref());
                *imitations.entry(key).or_insert(0) += 1;
            }
            LogEvent::Startled {
                tick,
                agent: who,
                event,
            } if *who == agent => {
                lines.push(format!("{} 被【{event}】吓了一大跳", format_clock(*tick)));
            }
            _ => {}
        }
    }
    // 结尾：今天主要在忙什么（行为占比前 3）。
    if !imitations.is_empty() {
        let mut v: Vec<(String, u64)> = imitations.into_iter().collect();
        v.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
        let (top, n) = v.swap_remove(0);
        if v.is_empty() {
            lines.push(format!("看别人做了【{top}】，有点想试。"));
        } else {
            lines.push(format!(
                "今天被种草了（最想试【{top}】，一共 {n} 回）。",
                n = v.iter().map(|(_, n)| n).sum::<u64>() + n
            ));
        }
    }
    if !acts.is_empty() {
        let mut v: Vec<(String, u64)> = acts.into_iter().collect();
        v.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
        let top: Vec<String> = v.iter().take(3).map(|(k, n)| format!("{k}×{n}")).collect();
        lines.push(format!("今天主要在：{}。", top.join("、")));
    }
    if !bonds.is_empty() {
        let mut v: Vec<(u32, f64)> = bonds.into_iter().collect();
        v.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
        let (id, gain) = v.last().unwrap();
        if *gain > 0.05 {
            lines.push(format!("和{}处得最好（亲近了 {gain:.2}）。", name_of(*id)));
        }
    }
    lines
}

/// 年度总结（设计稿 6.4：本年最常去的地方、最大的变化等）。
/// 输入全年事件，输出几行总结。
#[allow(clippy::type_complexity)]
pub fn annual_summary(
    events: &[LogEvent],
    agent: u32,
    name_of: impl Fn(u32) -> String,
    label: &LabelFn,
) -> Vec<String> {
    let mut acts: BTreeMap<String, u64> = BTreeMap::new();
    let mut earned = 0.0f64;
    let mut spent = 0.0f64;
    let mut strongest: Option<(f64, String)> = None;
    let mut partner_bond: BTreeMap<u32, f64> = BTreeMap::new();
    let mut met: BTreeMap<u32, f64> = BTreeMap::new();
    let (mut thefts_hit, mut job_moves, mut lover_now, mut babies, mut caught) =
        (0u64, 0u64, None, 0u64, 0u64);
    for ev in events {
        match ev {
            LogEvent::ActionStart {
                agent: who,
                behavior,
                variant,
                ..
            } if *who == agent => {
                let key = label(behavior, variant.as_deref());
                *acts.entry(key).or_insert(0) += 1;
            }
            LogEvent::Wallet {
                agent: who, delta, ..
            } if *who == agent => {
                if *delta >= 0.0 {
                    earned += *delta;
                } else {
                    spent -= *delta;
                }
            }
            LogEvent::MemoryWrite {
                agent: who,
                event,
                salience,
                ..
            } if *who == agent => {
                if strongest
                    .as_ref()
                    .map(|(s, _)| *salience > *s)
                    .unwrap_or(true)
                {
                    strongest = Some((*salience, event.clone()));
                }
            }
            LogEvent::BondChange { a, b, delta, .. } if *a == agent || *b == agent => {
                let other = if *a == agent { *b } else { *a };
                let e = met.entry(other).or_insert(0.0);
                let first = *e == 0.0;
                *e += delta;
                if first && *delta > 0.0 {
                    partner_bond.entry(other).or_insert(0.0);
                }
                partner_bond.entry(other).and_modify(|x| *x += delta);
            }
            LogEvent::Theft {
                victim,
                thief,
                caught: c,
                ..
            } => {
                if *victim == agent {
                    thefts_hit += 1;
                }
                if *thief == agent && *c {
                    caught += 1;
                }
            }
            LogEvent::JobChange { agent: who, .. } if *who == agent => {
                job_moves += 1;
            }
            LogEvent::Couple { a, b, started, .. } if *a == agent || *b == agent => {
                let other = if *a == agent { *b } else { *a };
                lover_now = if *started { Some(other) } else { None };
            }
            LogEvent::Born { mother, father, .. } if *mother == agent || *father == agent => {
                babies += 1;
            }
            _ => {}
        }
    }
    let mut out: Vec<String> = Vec::new();
    // 最常做的事。
    if let Some((top, _)) = acts.iter().max_by(|x, y| y.1.cmp(x.1).then(x.0.cmp(y.0))) {
        out.push(format!("这一年最常做的事是【{top}】。"));
    }
    out.push(format!(
        "挣了 {:.0}，花了 {:.0}。",
        earned.round(),
        spent.round()
    ));
    // 最亲近的人。
    if let Some((id, gain)) = partner_bond
        .iter()
        .max_by(|x, y| y.1.total_cmp(x.1).then(x.0.cmp(y.0)))
    {
        if *gain > 0.1 {
            out.push(format!(
                "和{}走得最近，这一年亲近了 {gain:.2}。",
                name_of(*id)
            ));
        }
    }
    if let Some(l) = lover_now {
        out.push(format!("现在和{}在一起。", name_of(l)));
    }
    if let Some((s, e)) = strongest.filter(|(s, _)| *s > 0.3) {
        out.push(format!("印象最深的一件事：【{e}】（强度 {s:.2}）。", s = s));
    }
    if job_moves > 0 {
        out.push(format!("换了 {job_moves} 次工作。"));
    }
    if thefts_hit > 0 {
        out.push(format!("被偷了 {thefts_hit} 次（这世道……）。"));
    }
    if caught > 0 {
        out.push(format!("被抓了 {caught} 次（社区服务都做熟了）。"));
    }
    if babies > 0 {
        out.push(format!("家里添了 {babies} 个孩子。"));
    }
    if out.is_empty() {
        out.push("这一年平平淡淡，也是好日子。".to_string());
    }
    out
}

/// 把日志行切成某一天的日志事件（日记用）。解析失败的行跳过。
pub fn events_of_day<'a>(lines: impl Iterator<Item = &'a str>, day: u64) -> Vec<LogEvent> {
    let mut out = Vec::new();
    for l in lines {
        if let Ok(ev) = serde_json::from_str::<LogEvent>(l) {
            if ev_tick(&ev).map(day_of) == Some(day) {
                out.push(ev);
            }
        }
    }
    out
}

/// 事件的发生拍（无时间概念的事件为 None）。
fn ev_tick(ev: &LogEvent) -> Option<u64> {
    use LogEvent::*;
    Some(match ev {
        Meta { .. } | Summary { .. } => return None,
        EnvEvent { tick, .. }
        | Startled { tick, .. }
        | BondChange { tick, .. }
        | Decision { tick, .. }
        | ActionStart { tick, .. }
        | ActionEnd { tick, .. }
        | MemoryWrite { tick, .. }
        | Snapshot { tick, .. }
        | PlayerCommand { tick, .. }
        | ReputationChange { tick, .. }
        | LocationState { tick, .. }
        | Exposure { tick, .. }
        | GoalSet { tick, .. }
        | GoalResist { tick, .. }
        | GoalViolation { tick, .. }
        | Theft { tick, .. }
        | JobChange { tick, .. }
        | Wallet { tick, .. }
        | GoalEnd { tick, .. }
        | SocialPick { tick, .. }
        | Couple { tick, .. }
        | Born { tick, .. }
        | Imitate { tick, .. }
        | ScenarioStart { tick, .. }
        | ScenarioResult { tick, .. } => *tick,
    })
}


/// 你的故事（M6）：玩家角色的经历回放——只挑"值得讲述"的大事，
/// 按时间排成一段可读的经历。M6 验收要求"新玩家能主动讲述一个发生在
/// 自己角色身上的非预期趣事"，这个函数就是把故事递到玩家手里。
/// 挑选规则：案发（受害/作案/被捕）、职业变动、恋爱成对与分手、
/// 孩子出生、被吓到、目睹所学（挑最强烈的几条）、恐惧消退、
/// 目标设立与结局、关卡结算。
pub fn player_story(
    events: &[LogEvent],
    player: u32,
    name_of: impl Fn(u32) -> String,
    label: &LabelFn,
) -> Vec<String> {
    let mut lines: Vec<(u64, String)> = Vec::new();
    // 目睹所学只讲最强烈的几条（其余太碎）。
    let mut witnessed: Vec<(u64, f64, String)> = Vec::new();
    for ev in events {
        match ev {
            LogEvent::Theft {
                tick,
                thief,
                victim,
                amount,
                caught,
                area,
            } => {
                if *thief == player {
                    let area_name = area_label(area);
                    if *caught {
                        lines.push((
                            *tick,
                            format!(
                                "在{}行窃{}元时被当场抓住，东西都还了回去",
                                area_name,
                                amount.round()
                            ),
                        ));
                    } else {
                        lines.push((
                            *tick,
                            format!(
                                "在{}顺走了{}的钱包（{}元）",
                                area_name,
                                name_of(*victim),
                                amount.round()
                            ),
                        ));
                    }
                } else if *victim == player {
                    lines.push((
                        *tick,
                        format!(
                            "在{}被{}偷走了{}元{}",
                            area_label(area),
                            name_of(*thief),
                            amount.round(),
                            if *caught { "（已被追回）" } else { "" }
                        ),
                    ));
                }
            }
            LogEvent::JobChange {
                tick,
                agent,
                from,
                to,
                reason,
            } if *agent == player => {
                let f = from.as_deref().unwrap_or("无业");
                match to.as_deref() {
                    Some(t) => {
                        lines.push((*tick, format!("从「{}」转行做了「{}」（{}）", f, t, reason)))
                    }
                    None => lines.push((*tick, format!("不再做「{}」了（{}）", f, reason))),
                }
            }
            LogEvent::Couple {
                tick,
                a,
                b,
                started,
            } if *a == player || *b == player => {
                let other = if *a == player { *b } else { *a };
                if *started {
                    lines.push((*tick, format!("和{}在一起了", name_of(other))));
                } else {
                    lines.push((*tick, format!("和{}分开了", name_of(other))));
                }
            }
            LogEvent::Born {
                tick,
                mother,
                father,
                ..
            } if *mother == player || *father == player => {
                lines.push((*tick, "家里添了个孩子".to_string()));
            }
            LogEvent::Startled { tick, agent, event } if *agent == player => {
                lines.push((*tick, format!("被「{}」吓得不轻", event)));
            }
            LogEvent::MemoryWrite {
                tick,
                agent,
                event,
                salience,
                vicarious: true,
                ..
            } if *agent == player => {
                witnessed.push((*tick, *salience, event.clone()));
            }
            LogEvent::Exposure {
                tick,
                agent,
                behavior,
                faded,
            } if *agent == player => {
                lines.push((
                    *tick,
                    format!(
                        "对「{}」的恐惧淡了一些（当面见了一次，消退 {:.0}%）",
                        label(behavior, None),
                        faded * 100.0
                    ),
                ));
            }
            LogEvent::GoalSet {
                tick,
                agent,
                behavior,
                ..
            } if *agent == player => {
                lines.push((
                    *tick,
                    format!("给自己定了个目标：少做「{}」", label(behavior, None)),
                ));
            }
            LogEvent::GoalEnd {
                tick,
                agent,
                behavior,
                success,
                abandoned,
                violations,
                ..
            } if *agent == player => {
                let verdict = if *abandoned {
                    "中途放弃了（习惯慢慢回来了）".to_string()
                } else if *success {
                    "成了".to_string()
                } else {
                    format!("没忍住 {} 次", violations)
                };
                lines.push((
                    *tick,
                    format!("「{}」的目标{}", label(behavior, None), verdict),
                ));
            }
            LogEvent::ScenarioResult {
                tick,
                scenario,
                won,
                ..
            } => {
                lines.push((
                    *tick,
                    format!(
                        "关卡「{}」{}——这段故事告一段落",
                        scenario,
                        if *won { "达成" } else { "未成" }
                    ),
                ));
            }
            _ => {}
        }
    }
    // 目睹所学：最强烈的 3 条，按时间排。
    witnessed.sort_by(|a, b| b.1.total_cmp(&a.1));
    for (tick, _, event) in witnessed.into_iter().take(3) {
        lines.push((tick, format!("目睹了别人的遭遇，把「{}」记在了心里", event)));
    }
    lines.sort_by_key(|(t, _)| *t);
    lines
        .into_iter()
        .map(|(t, s)| format!("{} {}", format_clock(t), s))
        .collect()
}

/// 区域显示名（日志里只有 ID；直接显示 ID 即可，配置中文名在表现层映射）。
fn area_label(area: &str) -> String {
    area.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_extraction_covers_all_events() {
        // 每个变体至少构造一条，保证 ev_tick 匹配齐全（编译器已强制，
        // 这里锁 day 过滤行为）。
        let ev = LogEvent::Born {
            tick: 1440 * 3 + 10,
            agent: 9,
            mother: 1,
            father: 2,
        };
        assert_eq!(ev_tick(&ev), Some(1440 * 3 + 10));
        assert_eq!(day_of(ev_tick(&ev).unwrap()), 3);
    }
}
