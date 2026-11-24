//! Export checks. Blocking problems must be fixed before an SRT is written;
//! overlaps are reported so the user can choose how to resolve them.

use serde::Serialize;

use super::{srt::normalize_text, Cue};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    EmptyText,
    NegativeTime,
    InvalidDuration,
    OutOfOrder,
    Overlap,
}

impl IssueKind {
    pub fn is_blocking(self) -> bool {
        !matches!(self, IssueKind::Overlap | IssueKind::OutOfOrder)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub kind: IssueKind,
    /// The cue the issue is about (for overlaps: the earlier cue).
    pub cue_id: String,
    /// The second cue involved in an overlap or ordering issue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other_cue_id: Option<String>,
    /// 1-based position of `cue_id` in the list, for messages.
    pub position: usize,
    pub blocking: bool,
    /// For overlaps: how many milliseconds the two cues overlap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlap_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub issues: Vec<Issue>,
    pub cue_count: usize,
}

impl Report {
    pub fn has_blocking(&self) -> bool {
        self.issues.iter().any(|i| i.blocking)
    }
    #[cfg(test)]
    pub fn overlap_count(&self) -> usize {
        self.issues.iter().filter(|i| i.kind == IssueKind::Overlap).count()
    }
}

pub fn validate(cues: &[Cue]) -> Report {
    let mut issues = Vec::new();
    let push = |issues: &mut Vec<Issue>, kind: IssueKind, i: usize, other: Option<&Cue>, overlap: Option<i64>| {
        issues.push(Issue {
            kind,
            cue_id: cues[i].id.clone(),
            other_cue_id: other.map(|c| c.id.clone()),
            position: i + 1,
            blocking: kind.is_blocking(),
            overlap_ms: overlap,
        });
    };

    for (i, cue) in cues.iter().enumerate() {
        if normalize_text(&cue.text).is_empty() {
            push(&mut issues, IssueKind::EmptyText, i, None, None);
        }
        if cue.start_ms < 0 || cue.end_ms < 0 {
            push(&mut issues, IssueKind::NegativeTime, i, None, None);
        }
        if cue.end_ms <= cue.start_ms {
            push(&mut issues, IssueKind::InvalidDuration, i, None, None);
        }
        if let Some(next) = cues.get(i + 1) {
            if next.start_ms < cue.start_ms {
                push(&mut issues, IssueKind::OutOfOrder, i, Some(next), None);
            } else if next.start_ms < cue.end_ms {
                push(&mut issues, IssueKind::Overlap, i, Some(next), Some(cue.end_ms - next.start_ms));
            }
        }
    }
    Report { issues, cue_count: cues.len() }
}

/// Sorts by start time (stable, so equal starts keep their order).
pub fn sort_by_start(cues: &mut [Cue]) {
    cues.sort_by_key(|c| (c.start_ms, c.end_ms));
}

/// Resolves overlaps by ending each cue where the next one starts.
/// Cues that would become shorter than `min_ms` instead push the next cue's start later
/// (never past that cue's own end). Input is sorted first.
pub fn fix_overlaps(cues: &mut Vec<Cue>, min_ms: i64) {
    sort_by_start(cues);
    for i in 0..cues.len().saturating_sub(1) {
        let (left, right) = cues.split_at_mut(i + 1);
        let cur = &mut left[i];
        let next = &mut right[0];
        if next.start_ms >= cur.end_ms {
            continue;
        }
        if next.start_ms - cur.start_ms >= min_ms {
            cur.end_ms = next.start_ms;
        } else {
            cur.end_ms = cur.start_ms + min_ms;
            next.start_ms = cur.end_ms.min(next.end_ms - 1).max(next.start_ms);
            if next.start_ms < cur.end_ms {
                // The next cue is too short to move; let the earlier cue end where it starts.
                cur.end_ms = next.start_ms.max(cur.start_ms + 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(id: &str, s: i64, e: i64, t: &str) -> Cue {
        Cue { id: id.into(), start_ms: s, end_ms: e, text: t.into() }
    }

    #[test]
    fn clean_list_has_no_issues() {
        let r = validate(&[cue("a", 0, 1000, "x"), cue("b", 1000, 2000, "y")]);
        assert!(r.issues.is_empty());
        assert!(!r.has_blocking());
    }

    #[test]
    fn detects_blocking_problems() {
        let r = validate(&[cue("a", 0, 1000, "   \n "), cue("b", 2000, 2000, "y"), cue("c", -5, 100, "z")]);
        let kinds: Vec<IssueKind> = r.issues.iter().map(|i| i.kind).collect();
        assert!(kinds.contains(&IssueKind::EmptyText));
        assert!(kinds.contains(&IssueKind::InvalidDuration));
        assert!(kinds.contains(&IssueKind::NegativeTime));
        assert!(r.has_blocking());
    }

    #[test]
    fn reports_overlaps_with_amount_and_is_not_blocking() {
        let r = validate(&[cue("a", 0, 1500, "x"), cue("b", 1200, 2000, "y")]);
        assert_eq!(r.overlap_count(), 1);
        let i = &r.issues[0];
        assert_eq!((i.cue_id.as_str(), i.other_cue_id.as_deref(), i.overlap_ms), ("a", Some("b"), Some(300)));
        assert!(!r.has_blocking());
    }

    #[test]
    fn reports_out_of_order() {
        let r = validate(&[cue("a", 5000, 6000, "x"), cue("b", 1000, 2000, "y")]);
        assert_eq!(r.issues[0].kind, IssueKind::OutOfOrder);
    }

    #[test]
    fn fix_trims_earlier_cue() {
        let mut v = vec![cue("a", 0, 1500, "x"), cue("b", 1200, 2000, "y")];
        fix_overlaps(&mut v, 300);
        assert_eq!((v[0].end_ms, v[1].start_ms), (1200, 1200));
        assert!(validate(&v).issues.is_empty());
    }

    #[test]
    fn fix_keeps_minimum_duration_by_moving_next_start() {
        let mut v = vec![cue("a", 1000, 3000, "x"), cue("b", 1100, 4000, "y")];
        fix_overlaps(&mut v, 500);
        assert_eq!((v[0].start_ms, v[0].end_ms), (1000, 1500));
        assert_eq!(v[1].start_ms, 1500);
        assert!(validate(&v).issues.is_empty());
    }

    #[test]
    fn fix_sorts_and_handles_identical_starts() {
        let mut v = vec![cue("b", 2000, 3000, "y"), cue("a", 2000, 2500, "x"), cue("c", 0, 100, "z")];
        fix_overlaps(&mut v, 200);
        assert_eq!(v[0].id, "c");
        assert!(validate(&v).issues.iter().all(|i| i.kind != IssueKind::Overlap), "{:?}", validate(&v));
        assert!(v.iter().all(|c| c.end_ms > c.start_ms));
    }
}
