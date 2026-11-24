//! Page breaks for `md2pdf --smart`.
//!
//! The page measures its blocks (`SMART_JS` in `md_to_pdf.rs`), this module
//! chooses where pages end, and the page then forces exactly those breaks.
//! Everything here is pure: positions are layout pixels from the top of the
//! document, the page height is given.
//!
//! The choice minimises a cost over the whole document (dynamic programming
//! over every place a page may end): empty space at the bottom of a page
//! costs quadratically, a break before a heading is rewarded, a break that
//! separates things that belong together is expensive or forbidden.

use serde::{Deserialize, Serialize};

/// What a block is. Unknown names from the page read as `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Heading,
    /// A paragraph.
    Text,
    /// A list item.
    Item,
    Code,
    Table,
    /// Block quote, callout, footnote.
    Quote,
    /// Image, diagram.
    Figure,
    Rule,
    /// `<!-- pagebreak -->`: the next block starts a page.
    Break,
    /// The title page: the next block starts a page.
    Title,
    #[default]
    #[serde(other)]
    Other,
}

/// One measured block, in document order.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Block {
    pub kind: Kind,
    /// Heading level 1–6, else 0.
    pub level: u8,
    pub top: f64,
    pub bottom: f64,
    /// Bottom edge of every text line (absolute), when measured.
    pub lines: Vec<f64>,
    /// Bottom edge of every body row of a table, when measured.
    pub rows: Vec<f64>,
    /// Height of the table head.
    pub head: f64,
    /// The table can be cut into separate tables with a repeated head.
    pub physical: bool,
    /// A paragraph ending in a colon: it introduces what follows.
    pub intro: bool,
    /// Items of one list share an id; -1 otherwise.
    pub list: i64,
}

impl Default for Block {
    fn default() -> Self {
        Block {
            kind: Kind::Other,
            level: 0,
            top: 0.0,
            bottom: 0.0,
            lines: Vec::new(),
            rows: Vec::new(),
            head: 0.0,
            physical: false,
            intro: false,
            list: -1,
        }
    }
}

/// What the page reports.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Measured {
    /// Height of one line of body text.
    pub line: f64,
    pub blocks: Vec<Block>,
}

/// Where a page may end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum At {
    Start,
    /// Before block `i`.
    Before(usize),
    /// Inside block `block`, after its line or row `after` (0-based).
    Cut { block: usize, after: usize, natural: bool },
    End,
}

/// The chosen breaks.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Plan {
    /// Blocks that start a page.
    pub breaks: Vec<usize>,
    /// The page (0-based) each of `breaks` starts.
    #[serde(skip)]
    pub break_pages: Vec<usize>,
    /// Tables to cut: (block, rows after which a new page starts).
    pub splits: Vec<(usize, Vec<usize>)>,
    /// Blocks the print engine may break inside.
    pub loose: Vec<usize>,
    /// Number of pages.
    pub pages: usize,
}

/// Lines a paragraph keeps on each side of a break.
pub const ORPHANS: usize = 3;
/// Lines of text that must follow a heading on its page.
pub const FOLLOW_LINES: f64 = 3.0;
/// Reward for starting a page with a heading of level 1, 2, 3.
pub const HEADING_BONUS: [f64; 3] = [20.0, 9.0, 3.0];
/// Empty space at the end of a page that a heading follows costs
/// `WHITESPACE * (empty share)²`.
pub const WHITESPACE: f64 = 100.0;
/// Beyond `WIDE_FROM` empty share even a heading does not justify the room:
/// `WIDE_SQUARE * (share - WIDE_FROM)²` on top.
pub const WIDE_FROM: f64 = 0.3;
pub const WIDE_SQUARE: f64 = 400.0;
/// Empty space before anything else is not wanted at all: it costs
/// `LOOSE_SQUARE * share² + LOOSE_LINEAR * share`, so the plan fills such
/// pages and leaves the room where a section ends.
pub const LOOSE_SQUARE: f64 = 200.0;
pub const LOOSE_LINEAR: f64 = 30.0;
pub const COST_INTRO: f64 = 15.0;
pub const COST_SPLIT_TEXT: f64 = 3.0;
pub const COST_SPLIT_BLOCK: f64 = 6.0;
pub const COST_LIST: f64 = 1.0;
pub const COST_BEFORE_RULE: f64 = 6.0;
pub const COST_AFTER_RULE: f64 = 2.0;
/// A last page filled less than this share costs `COST_RUNT`.
pub const RUNT_SHARE: f64 = 0.15;
pub const COST_RUNT: f64 = 30.0;
/// Taken when nothing valid fits (a block higher than a page).
pub const COST_OVERFLOW: f64 = 50.0;

#[derive(Debug, Clone, Copy)]
struct Cand {
    at: At,
    /// Where the content of a page ending here stops.
    end_y: f64,
    /// Where the next page's content starts.
    start_y: f64,
    /// Height added at the top of the next page (a repeated table head).
    extra: f64,
    /// For a natural cut: the next place the same block could break.
    next_y: f64,
    mandatory: bool,
    /// Inside the stretch a heading keeps with it.
    zone: bool,
    /// The next page starts with a heading.
    heading: bool,
    cost: f64,
}

fn is_break_after(k: Kind) -> bool {
    matches!(k, Kind::Break | Kind::Title)
}

/// The inner cuts of block `b`: (index after which, y, natural, extra).
fn cuts_of(b: &Block, page: f64) -> Vec<(usize, f64, bool, f64)> {
    let height = b.bottom - b.top;
    match b.kind {
        Kind::Text => {
            let n = b.lines.len();
            if n < 2 * ORPHANS {
                return Vec::new();
            }
            (ORPHANS - 1..=n - ORPHANS - 1).map(|k| (k, b.lines[k], true, 0.0)).collect()
        }
        Kind::Heading | Kind::Rule | Kind::Break | Kind::Title | Kind::Figure => Vec::new(),
        _ if height <= page => Vec::new(),
        Kind::Table => {
            let n = b.rows.len();
            if n < 4 {
                return Vec::new();
            }
            let extra = if b.physical { b.head } else { 0.0 };
            (1..=n - 3).map(|k| (k, b.rows[k], !b.physical, extra)).collect()
        }
        _ => {
            let n = b.lines.len();
            if n < 4 {
                return Vec::new();
            }
            (1..=n - 3).map(|k| (k, b.lines[k], true, 0.0)).collect()
        }
    }
}

fn candidates(m: &Measured, page: f64) -> Vec<Cand> {
    let blocks = &m.blocks;
    let mut out = vec![Cand {
        at: At::Start,
        end_y: 0.0,
        start_y: 0.0,
        extra: 0.0,
        next_y: 0.0,
        mandatory: false,
        zone: false,
        heading: false,
        cost: 0.0,
    }];
    // The content end before block i: the last block above that has height.
    let mut content_end = 0.0f64;
    for (i, b) in blocks.iter().enumerate() {
        if i > 0 && b.kind != Kind::Break {
            let prev = &blocks[i - 1];
            let mut cost = 0.0;
            if b.kind == Kind::Heading && prev.kind != Kind::Heading && (1..=3).contains(&b.level) {
                cost -= HEADING_BONUS[b.level as usize - 1];
            }
            if b.kind == Kind::Item && prev.kind == Kind::Item && b.list == prev.list {
                cost += COST_LIST;
            }
            if prev.intro && matches!(b.kind, Kind::Item | Kind::Table | Kind::Code | Kind::Figure | Kind::Quote) {
                cost += COST_INTRO;
            }
            if b.kind == Kind::Rule {
                cost += COST_BEFORE_RULE;
            }
            if prev.kind == Kind::Rule {
                cost += COST_AFTER_RULE;
            }
            let mandatory = is_break_after(prev.kind);
            out.push(Cand {
                at: At::Before(i),
                end_y: content_end,
                start_y: b.top,
                extra: 0.0,
                next_y: 0.0,
                mandatory,
                zone: false,
                heading: b.kind == Kind::Heading,
                cost: if mandatory { 0.0 } else { cost },
            });
        }
        let cuts = cuts_of(b, page);
        for (n, &(after, y, natural, extra)) in cuts.iter().enumerate() {
            let next_y = cuts.get(n + 1).map_or(b.bottom, |c| c.1);
            out.push(Cand {
                at: At::Cut { block: i, after, natural },
                end_y: y,
                start_y: y,
                extra,
                next_y,
                mandatory: false,
                zone: false,
                heading: false,
                cost: if b.kind == Kind::Text { COST_SPLIT_TEXT } else { COST_SPLIT_BLOCK },
            });
        }
        if b.kind != Kind::Break {
            content_end = content_end.max(b.bottom);
        }
    }
    out.push(Cand {
        at: At::End,
        end_y: content_end,
        start_y: content_end,
        extra: 0.0,
        next_y: 0.0,
        mandatory: false,
        zone: false,
        heading: false,
        cost: 0.0,
    });
    mark_zones(m, &mut out);
    out
}

/// A heading keeps what follows it: no page may end between the heading and
/// the point where `FOLLOW_LINES` of the next content are on the page (a
/// shorter block counts in full; list items count together).
fn mark_zones(m: &Measured, cands: &mut [Cand]) {
    let blocks = &m.blocks;
    let line = if m.line > 0.0 { m.line } else { 24.0 };
    for (i, b) in blocks.iter().enumerate() {
        if b.kind != Kind::Heading {
            continue;
        }
        // The first block after the run of headings.
        let Some(j) = (i + 1..blocks.len()).find(|&k| blocks[k].kind != Kind::Heading) else {
            continue;
        };
        let follow = &blocks[j];
        if is_break_after(follow.kind) {
            continue;
        }
        let target = follow.top + FOLLOW_LINES * line;
        let bottom = if follow.kind == Kind::Item {
            let last = (j..blocks.len())
                .take_while(|&k| blocks[k].kind == Kind::Item && blocks[k].list == follow.list)
                .last()
                .unwrap_or(j);
            blocks[last].bottom
        } else {
            follow.bottom
        };
        let limit = target.min(bottom);
        for c in cands.iter_mut() {
            let after_heading = match c.at {
                At::Before(k) => k > i,
                At::Cut { block, .. } => block > i,
                _ => false,
            };
            if after_heading && !c.mandatory && c.end_y < limit - 0.5 {
                c.zone = true;
            }
        }
    }
}

/// Choose the page breaks. `safety` is kept free at the bottom of every page
/// that ends at a block boundary (rounding differences of the print engine
/// must not push a block onto a page of its own).
pub fn plan(m: &Measured, page: f64, safety: f64) -> Plan {
    if m.blocks.is_empty() || page <= 0.0 {
        return Plan { pages: usize::from(!m.blocks.is_empty()), ..Plan::default() };
    }
    let cands = candidates(m, page);
    let n = cands.len();
    let mut best = vec![f64::INFINITY; n];
    let mut from = vec![usize::MAX; n];
    best[0] = 0.0;
    for a in 0..n - 1 {
        if !best[a].is_finite() {
            continue;
        }
        let ca = cands[a];
        let mut found = false;
        // The first candidate outside a heading's zone, for the overflow case.
        let mut escape: Option<usize> = None;
        for b in a + 1..n {
            let cb = cands[b];
            let height = ca.extra + cb.end_y - ca.start_y;
            let is_end = cb.at == At::End;
            if escape.is_none() && (!cb.zone || cb.mandatory || is_end) {
                escape = Some(b);
            }
            let fits = height <= page + 0.01;
            if !fits && !cb.mandatory {
                break;
            }
            let natural = matches!(cb.at, At::Cut { natural: true, .. });
            let valid = if cb.mandatory {
                true
            } else if natural {
                // The engine breaks at the last line that fits.
                !cb.zone && ca.extra + cb.next_y - ca.start_y > page + 0.01
            } else {
                !cb.zone && height <= page - safety
            };
            if valid {
                found = true;
                let fill = (height / page).clamp(0.0, 1.0);
                let space = if cb.mandatory {
                    if fits { 0.0 } else { COST_OVERFLOW }
                } else if is_end {
                    if fill < RUNT_SHARE && a != 0 { COST_RUNT } else { 0.0 }
                } else if cb.heading {
                    let wide = (1.0 - fill - WIDE_FROM).max(0.0);
                    WHITESPACE * (1.0 - fill) * (1.0 - fill) + WIDE_SQUARE * wide * wide
                } else {
                    LOOSE_SQUARE * (1.0 - fill) * (1.0 - fill) + LOOSE_LINEAR * (1.0 - fill)
                };
                let total = best[a] + space + cb.cost;
                if total < best[b] {
                    best[b] = total;
                    from[b] = a;
                }
            }
            if cb.mandatory {
                break;
            }
        }
        if !found {
            let b = escape.unwrap_or(a + 1);
            let total = best[a] + COST_OVERFLOW + cands[b].cost;
            if total < best[b] {
                best[b] = total;
                from[b] = a;
            }
        }
    }
    // Walk back from the end.
    let mut path = Vec::new();
    let mut at = n - 1;
    while at != 0 && from[at] != usize::MAX {
        at = from[at];
        path.push(at);
    }
    path.reverse();
    // A stretch higher than the paper (one huge table row, a long figure)
    // is sliced by the engine: it takes as many pages as its height.
    let mut pages = 0usize;
    let mut prev = 0usize;
    let mut break_pages = Vec::new();
    for &i in path.iter().skip(1).chain(std::iter::once(&(n - 1))) {
        let height = cands[prev].extra + cands[i].end_y - cands[prev].start_y;
        pages += (((height - 0.01) / page).ceil() as usize).max(1);
        if matches!(cands[i].at, At::Before(_)) {
            break_pages.push(pages);
        }
        prev = i;
    }
    let mut plan = Plan { pages, break_pages, ..Plan::default() };
    for &i in &path {
        match cands[i].at {
            At::Before(b) => plan.breaks.push(b),
            At::Cut { block, natural: true, .. } => {
                if !plan.loose.contains(&block) {
                    plan.loose.push(block);
                }
            }
            At::Cut { block, after, natural: false } => match plan.splits.last_mut() {
                Some((b, rows)) if *b == block => rows.push(after),
                _ => plan.splits.push((block, vec![after])),
            },
            At::Start | At::End => {}
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: f64 = 900.0;

    fn block(kind: Kind, top: f64, height: f64) -> Block {
        Block { kind, top, bottom: top + height, ..Block::default() }
    }

    /// Stack blocks with a 16-px gap; paragraphs get their lines.
    fn doc(parts: &[(Kind, f64)]) -> Measured {
        let mut y = 0.0;
        let mut blocks = Vec::new();
        for &(kind, h) in parts {
            let mut b = block(kind, y, h);
            if kind == Kind::Text {
                let n = (h / 24.0).round() as usize;
                b.lines = (1..=n).map(|k| y + k as f64 * 24.0).collect();
            }
            if kind == Kind::Heading {
                b.level = 2;
            }
            blocks.push(b);
            y += h + 16.0;
        }
        Measured { line: 24.0, blocks }
    }

    fn para(lines: usize) -> (Kind, f64) {
        (Kind::Text, lines as f64 * 24.0)
    }

    #[test]
    fn a_document_that_fits_has_one_page_and_no_breaks() {
        let m = doc(&[(Kind::Heading, 40.0), para(5), para(8)]);
        let p = plan(&m, PAGE, 6.0);
        assert_eq!(p, Plan { pages: 1, ..Plan::default() });
    }

    #[test]
    fn an_empty_document_has_no_pages() {
        assert_eq!(plan(&Measured::default(), PAGE, 6.0).pages, 0);
    }

    #[test]
    fn a_heading_never_ends_a_page() {
        // 800 px of text, then a heading that would still fit, then a table.
        let m = doc(&[para(33), (Kind::Heading, 40.0), (Kind::Table, 300.0)]);
        let p = plan(&m, PAGE, 6.0);
        assert_eq!(p.breaks, vec![1], "the heading moves to the next page with its table");
        assert_eq!(p.pages, 2);
    }

    #[test]
    fn a_run_of_headings_stays_with_the_text_below() {
        let mut m = doc(&[para(32), (Kind::Heading, 40.0), (Kind::Heading, 30.0), para(12)]);
        m.blocks[1].level = 1;
        let p = plan(&m, PAGE, 6.0);
        assert_eq!(p.breaks, vec![1]);
    }

    #[test]
    fn a_heading_needs_three_lines_of_its_paragraph() {
        // After the heading only two lines fit: the paragraph may not be cut
        // there, so the heading goes over.
        let m = doc(&[(Kind::Code, 770.0), (Kind::Heading, 40.0), para(10)]);
        let p = plan(&m, PAGE, 6.0);
        assert_eq!(p.breaks, vec![1]);
        assert!(p.loose.is_empty());
    }

    #[test]
    fn a_short_paragraph_after_a_heading_counts_in_full() {
        // Heading + a two-line paragraph may end the page (level 4: no
        // reward for starting a page with it).
        let mut m = doc(&[(Kind::Code, 700.0), (Kind::Heading, 40.0), para(2), (Kind::Code, 500.0)]);
        m.blocks[1].level = 4;
        let p = plan(&m, PAGE, 6.0);
        assert_eq!(p.breaks, vec![3]);
    }

    #[test]
    fn a_block_that_fits_on_a_page_is_never_cut() {
        for kind in [Kind::Code, Kind::Table, Kind::Quote, Kind::Figure, Kind::Item] {
            let mut m = doc(&[para(25), (kind, 500.0), para(3)]);
            m.blocks[1].lines = (1..=20).map(|k| m.blocks[1].top + k as f64 * 25.0).collect();
            m.blocks[1].rows = m.blocks[1].lines.clone();
            let p = plan(&m, PAGE, 6.0);
            assert_eq!(p.breaks, vec![1], "{kind:?}");
            assert!(p.loose.is_empty() && p.splits.is_empty(), "{kind:?}");
        }
    }

    #[test]
    fn a_long_paragraph_is_cut_with_three_lines_on_each_side() {
        // 30 lines of text, then a 40-line paragraph: page one holds 37 lines.
        let m = doc(&[para(30), para(40)]);
        let p = plan(&m, PAGE, 6.0);
        assert_eq!(p.loose, vec![1]);
        assert!(p.breaks.is_empty());
        assert_eq!(p.pages, 2);
    }

    #[test]
    fn a_paragraph_is_not_cut_when_fewer_than_three_lines_fit() {
        // Two lines would fit after the first block.
        let m = doc(&[(Kind::Code, 830.0), para(10), para(20)]);
        let p = plan(&m, PAGE, 6.0);
        assert_eq!(p.breaks, vec![1]);
    }

    #[test]
    fn a_paragraph_of_five_lines_is_never_cut() {
        let m = doc(&[(Kind::Code, 800.0), para(5), para(30)]);
        let p = plan(&m, PAGE, 6.0);
        assert!(!p.loose.contains(&1));
    }

    #[test]
    fn a_table_higher_than_a_page_is_cut_between_rows_with_its_head() {
        let mut m = doc(&[para(4), (Kind::Table, 1500.0), para(3)]);
        let top = m.blocks[1].top;
        m.blocks[1].head = 30.0;
        m.blocks[1].physical = true;
        // 49 rows of 30 px below the head.
        m.blocks[1].rows = (1..=49).map(|k| top + 30.0 + k as f64 * 30.0).collect();
        let p = plan(&m, PAGE, 6.0);
        assert_eq!(p.splits.len(), 1);
        let (b, rows) = &p.splits[0];
        assert_eq!(*b, 1);
        assert_eq!(rows.len(), 1, "two pages are enough");
        // What follows the cut must fit a page together with the head.
        let rest = m.blocks[1].bottom - m.blocks[1].rows[rows[0]];
        assert!(rest + 30.0 <= PAGE);
    }

    #[test]
    fn a_table_that_cannot_be_rebuilt_is_left_to_the_print_engine() {
        let mut m = doc(&[(Kind::Table, 1500.0)]);
        m.blocks[0].rows = (1..=50).map(|k| k as f64 * 30.0).collect();
        let p = plan(&m, PAGE, 6.0);
        assert!(p.splits.is_empty());
        assert_eq!(p.loose, vec![0]);
        assert_eq!(p.pages, 2);
    }

    #[test]
    fn a_chapter_starts_a_page_when_that_costs_little() {
        // Page one is 70 % full when the next chapter begins.
        let mut m = doc(&[para(26), (Kind::Heading, 40.0), para(30)]);
        m.blocks[1].level = 1;
        let p = plan(&m, PAGE, 6.0);
        assert_eq!(p.breaks, vec![1]);
        assert!(p.loose.is_empty());
    }

    #[test]
    fn a_chapter_does_not_leave_most_of_a_page_empty() {
        // Page one is 20 % full: the chapter continues on it.
        let mut m = doc(&[para(7), (Kind::Heading, 40.0), para(40)]);
        m.blocks[1].level = 1;
        let p = plan(&m, PAGE, 6.0);
        assert!(!p.breaks.contains(&1));
    }

    #[test]
    fn a_small_heading_is_worth_less_than_a_chapter() {
        // 60 % full: a chapter breaks, a level-3 heading does not.
        let mut m = doc(&[para(22), (Kind::Heading, 40.0), para(30)]);
        m.blocks[1].level = 1;
        assert!(plan(&m, PAGE, 6.0).breaks.contains(&1));
        m.blocks[1].level = 3;
        assert!(!plan(&m, PAGE, 6.0).breaks.contains(&1));
    }

    #[test]
    fn an_introducing_sentence_stays_with_its_list() {
        let mut m = doc(&[para(30), para(2), (Kind::Item, 30.0), (Kind::Item, 30.0), (Kind::Item, 30.0), (Kind::Item, 30.0), para(30)]);
        for b in &mut m.blocks[2..6] {
            b.list = 1;
        }
        // Without the colon the break may fall after the sentence.
        let free = plan(&m, PAGE, 6.0);
        m.blocks[1].intro = true;
        let kept = plan(&m, PAGE, 6.0);
        assert!(!kept.breaks.contains(&2), "{kept:?}");
        assert_ne!(free.breaks, vec![1], "the fixture must not break there anyway");
        assert!(kept.breaks.contains(&1) || kept.breaks.iter().all(|&b| b > 2));
    }

    #[test]
    fn room_is_left_where_a_section_ends_not_in_the_middle_of_one() {
        // Section one fills 77 % of the page. Five lines of section two
        // would still fit — the plan starts the section on a new page.
        let m = doc(&[(Kind::Heading, 40.0), para(14), para(12), (Kind::Heading, 40.0), para(12), para(8)]);
        let p = plan(&m, PAGE, 6.0);
        assert_eq!(p.breaks, vec![3], "{p:?}");
        assert!(p.loose.is_empty());
    }

    #[test]
    fn a_page_without_a_section_end_is_filled() {
        // No heading in sight: the page takes the six paragraphs that fit
        // instead of leaving room (two even pages would break before 4 or 5).
        let m = doc(&[para(5), para(5), para(5), para(5), para(5), para(5), para(5), para(5), para(4)]);
        let p = plan(&m, PAGE, 6.0);
        assert_eq!(p.breaks, vec![6], "{p:?}");
    }

    #[test]
    fn a_chapter_does_not_start_at_the_foot_of_a_page() {
        // 80 % full, then a chapter whose first lines would still fit.
        let mut m = doc(&[para(30), (Kind::Heading, 40.0), para(30)]);
        m.blocks[1].level = 1;
        assert_eq!(plan(&m, PAGE, 6.0).breaks, vec![1]);
    }

    #[test]
    fn the_reserve_keeps_a_block_off_the_very_end_of_a_page() {
        // The second block would end 2 px above the page's end.
        let m = doc(&[(Kind::Code, 600.0), (Kind::Code, 282.0)]);
        assert_eq!(plan(&m, PAGE, 0.0).pages, 1);
        let p = plan(&m, PAGE, 6.0);
        assert_eq!((p.pages, p.breaks), (2, vec![1]));
    }

    #[test]
    fn an_introducing_sentence_moves_over_with_its_code() {
        // The code block does not fit below the sentence. Without the colon
        // the sentence ends the page; with it, it goes along.
        let mut m = doc(&[para(30), para(2), (Kind::Code, 200.0)]);
        assert_eq!(plan(&m, PAGE, 6.0).breaks, vec![2]);
        m.blocks[1].intro = true;
        assert_eq!(plan(&m, PAGE, 6.0).breaks, vec![1]);
    }

    #[test]
    fn every_part_of_a_cut_table_fits_with_its_head() {
        // A tall head (200 px) and 58 rows of 30 px.
        let mut m = doc(&[(Kind::Table, 1940.0)]);
        m.blocks[0].head = 200.0;
        m.blocks[0].physical = true;
        m.blocks[0].rows = (1..=58).map(|k| 200.0 + k as f64 * 30.0).collect();
        let p = plan(&m, PAGE, 6.0);
        let cuts = &p.splits[0].1;
        let rows = &m.blocks[0].rows;
        let mut from = rows[cuts[0]];
        for &c in cuts[1..].iter().chain(std::iter::once(&57)) {
            assert!(rows[c] - from + 200.0 <= PAGE - 6.0 + 0.01, "part up to row {c} is {} high", rows[c] - from + 200.0);
            from = rows[c];
        }
        assert_eq!(p.pages, cuts.len() + 1);
    }

    #[test]
    fn a_manual_break_always_starts_a_page() {
        let m = doc(&[para(3), (Kind::Break, 0.0), para(3)]);
        let p = plan(&m, PAGE, 6.0);
        assert_eq!(p.breaks, vec![2]);
        assert_eq!(p.pages, 2);
    }

    #[test]
    fn a_title_page_is_a_page_of_its_own() {
        let m = doc(&[(Kind::Title, 400.0), (Kind::Heading, 40.0), para(5)]);
        let p = plan(&m, PAGE, 6.0);
        assert_eq!(p.breaks, vec![1]);
        assert_eq!(p.pages, 2);
    }

    #[test]
    fn a_manual_break_right_after_a_heading_is_obeyed() {
        let m = doc(&[(Kind::Heading, 40.0), (Kind::Break, 0.0), para(5)]);
        assert_eq!(plan(&m, PAGE, 6.0).breaks, vec![2]);
    }

    #[test]
    fn a_figure_higher_than_a_page_still_ends_the_plan() {
        let m = doc(&[para(3), (Kind::Figure, 1400.0), para(3)]);
        let p = plan(&m, PAGE, 6.0);
        assert!(p.pages >= 2);
        assert!(p.breaks.contains(&1), "it starts on a fresh page: {p:?}");
    }

    #[test]
    fn a_stretch_higher_than_the_paper_counts_every_page_it_takes() {
        // 2.6 pages of one figure between two paragraphs.
        let m = doc(&[para(3), (Kind::Figure, 2340.0), para(3)]);
        let p = plan(&m, PAGE, 6.0);
        assert_eq!(p.breaks, vec![1, 2]);
        assert_eq!(p.pages, 1 + 3 + 1);
    }

    #[test]
    fn no_page_is_higher_than_the_paper() {
        // A long mixed document: every page between forced breaks fits.
        let mut parts = Vec::new();
        for k in 0..60 {
            parts.push((Kind::Heading, 40.0));
            parts.push(para(2 + (k * 7) % 4));
            parts.push((if k % 3 == 0 { Kind::Code } else { Kind::Table }, 80.0 + ((k * 53) % 300) as f64));
            parts.push(para(1 + (k * 5) % 5));
        }
        let m = doc(&parts);
        let p = plan(&m, PAGE, 6.0);
        assert!(p.loose.is_empty(), "nothing here needs a cut");
        let mut start = 0.0;
        for &b in &p.breaks {
            let end = m.blocks[b - 1].bottom;
            assert!(end - start <= PAGE - 6.0 + 0.01, "page before block {b} is {} high", end - start);
            assert_ne!(m.blocks[b - 1].kind, Kind::Heading, "a heading ends the page before block {b}");
            start = m.blocks[b].top;
        }
        assert!(m.blocks.last().unwrap().bottom - start <= PAGE);
        assert_eq!(p.pages, p.breaks.len() + 1);
    }

    #[test]
    fn the_last_page_is_not_left_with_a_scrap() {
        let m = doc(&[para(32), para(4)]);
        let p = plan(&m, PAGE, 6.0);
        assert_eq!(p.pages, 1, "32 + 4 lines and one gap fit: {p:?}");
        // One line too many: rather move a paragraph than leave a single
        // line alone on the last page.
        let m = doc(&[para(26), para(10), para(1)]);
        let p = plan(&m, PAGE, 6.0);
        assert_eq!(p.breaks, vec![1], "better two balanced pages than a scrap: {p:?}");
    }

    #[test]
    fn list_items_break_between_items_not_inside() {
        let mut parts = vec![para(30)];
        parts.extend(std::iter::repeat_n((Kind::Item, 60.0), 10));
        let mut m = doc(&parts);
        for b in &mut m.blocks[1..] {
            b.list = 7;
        }
        let p = plan(&m, PAGE, 6.0);
        assert_eq!(p.breaks.len(), 1);
        assert!(p.loose.is_empty());
    }

    #[test]
    fn the_page_reports_unknown_kinds_as_other() {
        let m: Measured = serde_json::from_str(r#"{"line":24,"blocks":[{"kind":"heading","level":2,"top":0,"bottom":40},{"kind":"marquee","top":56,"bottom":90}]}"#).unwrap();
        assert_eq!(m.blocks[0].kind, Kind::Heading);
        assert_eq!(m.blocks[1].kind, Kind::Other);
        assert_eq!(m.blocks[1].list, -1);
    }
}
