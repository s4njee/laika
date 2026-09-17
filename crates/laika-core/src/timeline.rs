//! V18: timeline grouping — pure date sections + event segmentation.
//! The app memoizes over these structures per render generation;
//! rows flatten with collapse sets applied (no re-query on collapse).

use std::collections::{HashMap, HashSet};

/// V18: one dated photo (undated stamps are empty strings).
#[derive(Clone, Debug)]
pub struct TItem {
    pub id: i64,
    pub stamp: String,
    pub folder: String,
}

/// V18: parsed EXIF-style stamp (`YYYY:MM:DD HH:MM:SS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stamp {
    pub y: u32,
    pub m: u32,
    pub d: u32,
    pub hh: u32,
    pub mm: u32,
    pub ss: u32,
}

impl Stamp {
    /// Parse garbage-tolerantly; undated or malformed reads as None.
    /// Positional over the fixed `YYYY:MM:DD HH:MM:SS` (or `YYYY-MM-DD`) shape (no
    /// split/alloc in the per-frame hot path).
    pub fn parse(s: &str) -> Option<Stamp> {
        let b = s.as_bytes();
        // Date separators may be `:` (raw EXIF) or `-` (kamadak-exif's
        // display form, which is what import stores).
        if b.len() != 19
            || !matches!(b[4], b':' | b'-')
            || b[7] != b[4]
            || b[10] != b' '
            || b[13] != b':'
            || b[16] != b':'
        {
            return None;
        }
        fn num(b: &[u8], lo: usize, hi: usize) -> Option<u32> {
            std::str::from_utf8(&b[lo..hi]).ok()?.parse().ok()
        }
        let (y, m, d) = (num(b, 0, 4)?, num(b, 5, 7)?, num(b, 8, 10)?);
        let (hh, mm, ss) = (num(b, 11, 13)?, num(b, 14, 16)?, num(b, 17, 19)?);
        if y < 1900
            || !(1..=12).contains(&m)
            || !(1..=31).contains(&d)
            || hh > 23
            || mm > 59
            || ss > 60
        {
            return None;
        }
        Some(Stamp {
            y,
            m,
            d,
            hh,
            mm,
            ss,
        })
    }

    /// Rough epoch hours (civil days — leap years handled, leap seconds not).
    pub fn epoch_hours(&self) -> i64 {
        let y = self.y as i64;
        let m = self.m as i64;
        let d = self.d as i64;
        let era = if m <= 2 { y - 1 } else { y };
        let yoe = era % 400;
        let doy = (153 * (if m <= 2 { m + 12 } else { m } - 3) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era / 400 * 146097 + doe - 719468;
        days * 24 + self.hh as i64
    }

    /// Epoch seconds on the same civil-day basis as [`Stamp::epoch_hours`]
    /// (gap math must not truncate minutes: 10:00→14:59 is > 4h).
    fn epoch_secs(&self) -> i64 {
        self.epoch_hours() * 3600 + self.mm as i64 * 60 + self.ss as i64
    }

    pub fn day_key(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }

    pub fn month_key(&self) -> String {
        format!("{:04}-{:02}", self.y, self.m)
    }

    pub fn year_key(&self) -> String {
        format!("{:04}", self.y)
    }
}

/// V18: one capture-gap event (indices into the dated order).
#[derive(Clone, Debug)]
pub struct TEvent {
    pub start_idx: usize,
    pub end_idx: usize,
    pub count: usize,
    pub start: Stamp,
    pub end: Stamp,
    /// Suggested name: date range + most common folder leaf.
    pub name: String,
}

/// V18: grouped timeline (dated sections + undated tail + events).
#[derive(Clone, Debug, Default)]
pub struct Timeline {
    /// (year, months[(month, days[(day, ids)])]) — all ascending.
    pub years: Vec<(String, Vec<(String, Vec<(String, Vec<i64>)>)>)>,
    pub undated: Vec<i64>,
    pub events: Vec<TEvent>,
    pub total: usize,
}

/// V18: folder leaf for suggested names (`/a/b` → `b`).
pub fn folder_leaf(folder: &str) -> String {
    folder
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(folder)
        .to_string()
}

/// V18: build sections + events from capture order. Undated photos
/// trail at the end, outside events. Pure — unit-tested below.
pub fn build(items: &[TItem], gap_hours: u32) -> Timeline {
    let gap_secs = gap_hours.max(1) as i64 * 3600;
    let mut dated: Vec<(Stamp, &TItem)> = Vec::new();
    let mut undated = Vec::new();
    for item in items {
        match Stamp::parse(&item.stamp) {
            Some(s) => dated.push((s, item)),
            None => undated.push(item.id),
        }
    }
    dated.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
    // Sections — integer comparison in the hot loop; key strings only
    // materialize on group boundaries (50k distinct days stay fast).
    let mut years: Vec<(String, Vec<(String, Vec<(String, Vec<i64>)>)>)> = Vec::new();
    let mut cur: Option<(u32, u32, u32)> = None;
    for (stamp, item) in &dated {
        let ymd = (stamp.y, stamp.m, stamp.d);
        if cur != Some(ymd) {
            cur = Some(ymd);
            let (yk, mk, dk) = (stamp.year_key(), stamp.month_key(), stamp.day_key());
            if years.last().map(|(y, _)| y != &yk).unwrap_or(true) {
                years.push((yk.clone(), Vec::new()));
            }
            let months = &mut years.last_mut().unwrap().1;
            if months.last().map(|(m, _)| m != &mk).unwrap_or(true) {
                months.push((mk.clone(), Vec::new()));
            }
            months.last_mut().unwrap().1.push((dk.clone(), Vec::new()));
        }
        years
            .last_mut()
            .unwrap()
            .1
            .last_mut()
            .unwrap()
            .1
            .last_mut()
            .unwrap()
            .1
            .push(item.id);
    }
    // Events: a gap strictly greater than the threshold starts one.
    let mut events = Vec::new();
    let mut start = 0;
    for i in 1..dated.len() {
        if dated[i].0.epoch_secs() - dated[i - 1].0.epoch_secs() > gap_secs {
            events.push(make_event(&dated, start, i - 1));
            start = i;
        }
    }
    if !dated.is_empty() {
        events.push(make_event(&dated, start, dated.len() - 1));
    }
    let total = dated.len() + undated.len();
    Timeline {
        years,
        undated,
        events,
        total,
    }
}

fn make_event(dated: &[(Stamp, &TItem)], start: usize, end: usize) -> TEvent {
    let (s0, s1) = (dated[start].0, dated[end].0);
    let mut folders: HashMap<String, usize> = HashMap::new();
    for (_, item) in &dated[start..=end] {
        *folders.entry(folder_leaf(&item.folder)).or_default() += 1;
    }
    // Deterministic: highest count wins, ties break alphabetically.
    let top = folders
        .into_iter()
        .fold(None, |best: Option<(String, usize)>, cur| match best {
            None => Some(cur),
            Some(b) if cur.1 > b.1 || (cur.1 == b.1 && cur.0 < b.0) => Some(cur),
            Some(b) => Some(b),
        });
    let top = top.map(|(f, _)| f).unwrap_or_default();
    let day = s0.day_key();
    let name = if s1.day_key() == day && !top.is_empty() {
        format!("{day} · {top}")
    } else if s1.day_key() == day {
        day
    } else if !top.is_empty() {
        format!("{} → {} · {top}", day, s1.day_key())
    } else {
        format!("{} → {}", day, s1.day_key())
    };
    TEvent {
        start_idx: start,
        end_idx: end,
        count: end - start + 1,
        start: s0,
        end: s1,
        name,
    }
}

/// V18: flattened virtual rows (uniform height for `uniform_list`).
#[derive(Clone, Debug)]
pub enum Row {
    Year {
        key: String,
        count: usize,
        start: String,
        end: String,
        collapsed: bool,
    },
    Month {
        key: String,
        count: usize,
        collapsed: bool,
    },
    Day {
        key: String,
        count: usize,
        collapsed: bool,
    },
    Event {
        name: String,
        count: usize,
    },
    /// One thumb row (≤ cols ids; the view pads the tail).
    Thumbs(Vec<i64>),
    Undated {
        count: usize,
    },
}

/// V18: flatten with collapse applied. Collapsing hides rows without
/// re-query — the caller memoizes on (generation, collapse rev, gap).
/// Event banners show above the day containing the event start.
pub fn flatten(
    tl: &Timeline,
    cols: usize,
    collapsed_years: &HashSet<String>,
    collapsed_months: &HashSet<String>,
    collapsed_days: &HashSet<String>,
) -> Vec<Row> {
    let cols = cols.max(1);
    let mut rows = Vec::new();
    // Event start day → event index (banner above that day).
    let mut event_at: HashMap<String, usize> = HashMap::new();
    for (i, ev) in tl.events.iter().enumerate() {
        event_at.entry(ev.start.day_key()).or_insert(i);
    }
    for (yk, months) in &tl.years {
        let ycount: usize = months
            .iter()
            .map(|(_, d)| d.iter().map(|(_, ids)| ids.len()).sum::<usize>())
            .sum();
        let (start, end) = section_range(months);
        let ycol = collapsed_years.contains(yk);
        rows.push(Row::Year {
            key: yk.clone(),
            count: ycount,
            start,
            end,
            collapsed: ycol,
        });
        if ycol {
            continue;
        }
        for (mk, days) in months {
            let mcount: usize = days.iter().map(|(_, ids)| ids.len()).sum();
            let mcol = collapsed_months.contains(mk);
            rows.push(Row::Month {
                key: mk.clone(),
                count: mcount,
                collapsed: mcol,
            });
            if mcol {
                continue;
            }
            for (dk, ids) in days {
                if let Some(&ei) = event_at.get(dk) {
                    let ev = &tl.events[ei];
                    rows.push(Row::Event {
                        name: ev.name.clone(),
                        count: ev.count,
                    });
                }
                let dcol = collapsed_days.contains(dk);
                rows.push(Row::Day {
                    key: dk.clone(),
                    count: ids.len(),
                    collapsed: dcol,
                });
                if dcol {
                    continue;
                }
                for chunk in ids.chunks(cols) {
                    rows.push(Row::Thumbs(chunk.to_vec()));
                }
            }
        }
    }
    if !tl.undated.is_empty() {
        rows.push(Row::Undated {
            count: tl.undated.len(),
        });
        for chunk in tl.undated.chunks(cols) {
            rows.push(Row::Thumbs(chunk.to_vec()));
        }
    }
    rows
}

fn section_range(months: &[(String, Vec<(String, Vec<i64>)>)]) -> (String, String) {
    let mut first = String::new();
    let mut last = String::new();
    for (_, days) in months {
        for (dk, _) in days {
            if first.is_empty() {
                first = dk.clone();
            }
            last = dk.clone();
        }
    }
    (first, last)
}

/// V18: month labels → row index for the scrubber (month header rows).
pub fn month_rows(rows: &[Row]) -> Vec<(String, usize)> {
    rows.iter()
        .enumerate()
        .filter_map(|(i, r)| match r {
            Row::Month { key, .. } => Some((key.clone(), i)),
            _ => None,
        })
        .collect()
}

/// V18: row index of a day header (Jump to Date).
pub fn day_row(rows: &[Row], day: &str) -> Option<usize> {
    rows.iter()
        .position(|r| matches!(r, Row::Day { key, .. } if key == day))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: i64, stamp: &str, folder: &str) -> TItem {
        TItem {
            id,
            stamp: stamp.into(),
            folder: folder.into(),
        }
    }

    #[test]
    fn groups_years_months_days_and_orders() {
        let items = vec![
            item(3, "2026:09:12 10:00:00", "/pics/a"),
            item(1, "2025:01:02 08:00:00", "/pics/a"),
            item(2, "2026:09:12 09:00:00", "/pics/b"),
            item(4, "", "/pics/a"),
        ];
        let tl = build(&items, 4);
        assert_eq!(tl.total, 4);
        assert_eq!(tl.years.len(), 2);
        assert_eq!(tl.years[0].0, "2025");
        // Same day groups in capture order regardless of input order.
        let days = &tl.years[1].1[0].1;
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].1, vec![2, 3]);
        assert_eq!(tl.undated, vec![4]);
        // Two events (2025 day + 2026 day — the gap spans a year).
        assert_eq!(tl.events.len(), 2);
        assert_eq!(tl.events[1].name, "2026-09-12 · a");
    }

    #[test]
    fn gaps_segment_events_and_rerun_differs() {
        let items = vec![
            item(1, "2026:06:14 10:00:00", "/w/ceremony"),
            item(2, "2026:06:14 10:30:00", "/w/ceremony"),
            item(3, "2026:06:14 18:00:00", "/w/party"),
            item(4, "2026:06:15 09:00:00", "/w/party"),
        ];
        // 4h gap: morning, evening, next day.
        let tl = build(&items, 4);
        assert_eq!(
            tl.events.len(),
            3,
            "{:?}",
            tl.events.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
        assert_eq!(tl.events[0].count, 2);
        // 12h gap: wedding day merges, next morning splits.
        let tl = build(&items, 12);
        assert_eq!(tl.events.len(), 2);
        assert_eq!(tl.events[0].count, 3);
        assert!(
            tl.events[0].name.contains("2026-06-14"),
            "{}",
            tl.events[0].name
        );
    }

    #[test]
    fn parses_dashed_display_dates() {
        let a = Stamp::parse("2026-09-12 06:51:25").expect("dashed date");
        let b = Stamp::parse("2026:09:12 06:51:25").expect("colon date");
        assert_eq!(
            (a.y, a.m, a.d, a.hh, a.mm, a.ss),
            (b.y, b.m, b.d, b.hh, b.mm, b.ss)
        );
        assert!(Stamp::parse("2026-09:12 06:51:25").is_none());
    }

    #[test]
    fn gap_threshold_counts_minutes() {
        // 4h59m apart is over a 4h gap; exactly 4h is not.
        let tl = build(
            &[
                item(1, "2026:06:14 10:00:00", "/a"),
                item(2, "2026:06:14 14:59:00", "/a"),
            ],
            4,
        );
        assert_eq!(tl.events.len(), 2);
        let tl = build(
            &[
                item(1, "2026:06:14 10:59:00", "/a"),
                item(2, "2026:06:14 14:59:00", "/a"),
            ],
            4,
        );
        assert_eq!(tl.events.len(), 1);
    }

    #[test]
    fn collapse_hides_without_requery() {
        let items = vec![
            item(1, "2026:01:01 10:00:00", "/a"),
            item(2, "2026:02:01 10:00:00", "/a"),
            item(3, "2025:05:05 10:00:00", "/a"),
        ];
        let tl = build(&items, 4);
        let full = flatten(&tl, 4, &HashSet::new(), &HashSet::new(), &HashSet::new());
        assert!(full.len() > 3);
        // Collapse 2026: its month/day/thumb rows vanish, header stays.
        let cy: HashSet<String> = ["2026".into()].into_iter().collect();
        let collapsed = flatten(&tl, 4, &cy, &HashSet::new(), &HashSet::new());
        assert!(collapsed.len() < full.len());
        assert!(
            collapsed
                .iter()
                .any(|r| matches!(r, Row::Year { key, collapsed: true, .. } if key == "2026"))
        );
        assert!(
            !collapsed
                .iter()
                .any(|r| matches!(r, Row::Month { key, .. } if key.starts_with("2026")))
        );
        // Day collapse hides only its thumbs.
        let cd: HashSet<String> = ["2026-01-01".into()].into_iter().collect();
        let daycol = flatten(&tl, 4, &HashSet::new(), &HashSet::new(), &cd);
        assert!(
            daycol
                .iter()
                .any(|r| matches!(r, Row::Day { key, collapsed: true, .. } if key == "2026-01-01"))
        );
        // Month rows map + day lookup for scrubber/jump.
        let mr = month_rows(&full);
        assert!(mr.iter().any(|(k, _)| k == "2026-01"));
        assert!(day_row(&full, "2025-05-05").is_some());
        assert!(day_row(&full, "1999-01-01").is_none());
    }

    #[test]
    fn v18_scale_smoke() {
        // Clustered like a real catalog: 500 shoot days × 100 photos.
        // Structural scale gate (timing printed for the per-frame budget).
        let mut items = Vec::new();
        for i in 0..50_000 {
            let day_n = i / 100;
            let day = 1 + (day_n % 28);
            let mon = 1 + ((day_n / 28) % 12);
            let yr = 2020 + (day_n / 336);
            items.push(super::TItem {
                id: i as i64,
                stamp: format!("{yr:04}:{mon:02}:{day:02} 10:{:02}:00", i % 60),
                folder: format!("/pics/ev{}", i % 50),
            });
        }
        let t0 = std::time::Instant::now();
        let tl = super::build(&items, 4);
        let t1 = std::time::Instant::now();
        let rows = super::flatten(
            &tl,
            5,
            &std::collections::HashSet::new(),
            &std::collections::HashSet::new(),
            &std::collections::HashSet::new(),
        );
        let t2 = std::time::Instant::now();
        eprintln!(
            "50k: build={}ms flatten={}ms rows={} events={}",
            t1.duration_since(t0).as_millis(),
            t2.duration_since(t1).as_millis(),
            rows.len(),
            tl.events.len()
        );
        assert_eq!(tl.total, 50_000);
    }

    #[test]
    fn malformed_stamps_go_undated() {
        assert!(Stamp::parse("garbage").is_none());
        assert!(Stamp::parse("2026:13:01 10:00:00").is_none());
        assert!(Stamp::parse("2026:01:01").is_none());
        assert!(Stamp::parse("").is_none());
        let tl = build(&[item(1, "2026:13:99 99:99:99", "/a")], 4);
        assert_eq!(tl.undated, vec![1]);
        assert!(tl.years.is_empty() && tl.events.is_empty());
        assert_eq!(folder_leaf("/a/b/"), "b");
    }
}
