//! Finds output that only moved since the last frame sent to the remote host, so it can cross as a move
//! instead of as pixels. Scrolling repaints a whole viewport whose rows mostly reappear a few rows away.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use tinyland_remote::{Move, Rect, Size};

/// Passes a row hash through unchanged: map keys here are already hashes.
#[derive(Default)]
struct Identity(u64);

impl Hasher for Identity {
    fn finish(&self) -> u64 { self.0 }
    fn write(&mut self, bytes: &[u8]) { for &b in bytes { self.0 = self.0.rotate_left(8) ^ u64::from(b); } }
    fn write_u64(&mut self, n: u64) { self.0 = n; }
}

type HashedMap<V> = HashMap<u64, V, BuildHasherDefault<Identity>>;

/// Width of the column bands rows are hashed in. A scrollbar or other column that changes differently from the
/// scrolled content only spoils its own band.
const BAND: u32 = 64;
/// How far beyond a damaged rectangle its rows' earlier positions are looked for: damage often arrives as
/// stripes that a scroll moves rows between.
const SEARCH: u32 = 512;
/// The fewest rows worth sending as a move.
const MIN_ROWS: u32 = 32;
/// A row whose hash appears more often than this in the shipped band, such as a flat background, does not vote.
const MAX_CANDIDATES: usize = 4;
/// Frames planned without finding a move before planning backs off, and the frames it then skips. Video and
/// other content that never scrolls would otherwise pay for hashing every damaged row of every frame.
const MISSES_BEFORE_BACKOFF: u32 = 2;
const BACKOFF_FRAMES: u32 = 7;
/// Rectangles one damaged rectangle may become; with more, it is sent whole.
const MAX_PIECES: usize = 4;

/// The output as last sent to the host, and a hash of every row of every column band of it.
pub struct Shipped {
    size: Size,
    pixels: Vec<u8>,
    hashes: Vec<u64>,
    /// Row hashes of the frame being planned, for [`Shipped::record`] to keep; `None` where not computed.
    fresh: Vec<Option<u64>>,
    /// Hashes not updated since their pixels were recorded while planning was backed off.
    stale: Vec<bool>,
    misses: u32,
    skip: u32,
}

fn source(m: Move) -> Rect { Rect { x: m.x, y: m.src_y, width: m.width, height: m.height } }

fn intersects(a: Rect, b: Rect) -> bool { a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height }

fn hash(row: &[u8]) -> u64 {
    let (words, tail) = row.as_chunks::<8>();
    let mut h = 0x9e37_79b9_7f4a_7c15u64 ^ row.len() as u64;
    for word in words { h = (h ^ u64::from_le_bytes(*word)).wrapping_mul(0x0100_0000_01b3).rotate_left(23); }
    for &byte in tail { h = (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3); }
    h
}

impl Shipped {
    pub fn new(size: Size) -> Self {
        let mut shipped = Self { size, pixels: vec![0; size.pixel_len()], hashes: Vec::new(), fresh: Vec::new(), stale: Vec::new(), misses: 0, skip: 0 };
        shipped.hashes = (0..size.height()).flat_map(|y| (0..shipped.bands()).map(move |b| (y, b))).map(|(y, b)| shipped.band_hash(&shipped.pixels, y, b)).collect();
        shipped.fresh = vec![None; shipped.hashes.len()];
        shipped.stale = vec![false; shipped.hashes.len()];
        shipped
    }

    pub fn size(&self) -> Size { self.size }

    fn bands(&self) -> u32 { self.size.width().div_ceil(BAND) }

    fn row(&self, y: u32, x0: u32, x1: u32) -> std::ops::Range<usize> {
        let start = (y * self.size.width() + x0) as usize * 4;
        start..start + (x1 - x0) as usize * 4
    }

    fn band_hash(&self, pixels: &[u8], y: u32, band: u32) -> u64 {
        hash(&pixels[self.row(y, band * BAND, ((band + 1) * BAND).min(self.size.width()))])
    }

    /// Splits `rects`, the damage of `current` since the shipped output, into moves of shipped pixels and the
    /// rectangles still to send. Applying the moves to the shipped output, then the returned rectangles of
    /// `current`, reproduces `current` inside `rects`.
    pub fn plan(&mut self, current: &[u8], rects: &[Rect]) -> (Vec<Move>, Vec<Rect>) {
        if self.skip > 0 {
            self.skip -= 1;
            return (Vec::new(), rects.to_vec());
        }
        let (mut moves, mut rest) = (Vec::new(), Vec::new());
        for (i, &r) in rects.iter().enumerate() {
            let overlaps = rects.iter().enumerate().any(|(j, o)| j != i && intersects(r, *o));
            let planned = (!overlaps && r.height >= 2 * MIN_ROWS).then(|| self.plan_rect(current, r)).flatten();
            // Moves apply in order, so a source an earlier move overwrote no longer holds shipped pixels.
            let planned = planned.filter(|(found, _)| found.iter().all(|m| moves.iter().all(|earlier: &Move| !intersects(source(*m), earlier.destination()))));
            match planned {
                Some((found, pieces)) => { moves.extend(found); rest.extend(pieces); }
                None => rest.push(r),
            }
        }
        if moves.is_empty() {
            self.misses += 1;
            if self.misses >= MISSES_BEFORE_BACKOFF { self.misses = 0; self.skip = BACKOFF_FRAMES; }
        } else {
            self.misses = 0;
        }
        (moves, rest)
    }

    fn plan_rect(&mut self, current: &[u8], r: Rect) -> Option<(Vec<Move>, Vec<Rect>)> {
        let (first_band, last_band) = (r.x / BAND, (r.x + r.width - 1) / BAND);
        let bands = self.bands();
        let mut votes: HashMap<i64, u32> = HashMap::new();
        let mut previous: HashedMap<Vec<u32>> = HashedMap::default();
        let (lo, hi) = (r.y.saturating_sub(SEARCH), (r.y + r.height + SEARCH).min(self.size.height()));
        for band in first_band..=last_band {
            previous.clear();
            for y in lo..hi {
                let at = (y * bands + band) as usize;
                if std::mem::take(&mut self.stale[at]) { self.hashes[at] = self.band_hash(&self.pixels, y, band); }
                let rows = previous.entry(self.hashes[at]).or_default();
                if rows.len() <= MAX_CANDIDATES { rows.push(y); }
            }
            for y in r.y..r.y + r.height {
                let now = self.band_hash(current, y, band);
                self.fresh[(y * bands + band) as usize] = Some(now);
                let Some(rows) = previous.get(&now) else { continue };
                if rows.len() > MAX_CANDIDATES { continue; }
                for &from in rows.iter().filter(|&&from| from != y) { *votes.entry(i64::from(from) - i64::from(y)).or_default() += 1; }
            }
        }
        let (dy, count) = votes.into_iter().max_by_key(|&(dy, count)| (count, -dy.abs()))?;
        if count < MIN_ROWS { return None; }
        let columns = |band: u32| ((band * BAND).max(r.x), ((band + 1) * BAND).min(r.x + r.width));
        let same = |band: u32, y: u32| {
            let from = i64::from(y) + dy;
            if from < i64::from(lo) || from >= i64::from(hi) { return false; }
            let (x0, x1) = columns(band);
            current[self.row(y, x0, x1)] == self.pixels[self.row(from as u32, x0, x1)]
        };
        let moving: Vec<u32> = (first_band..=last_band).filter(|&b| (r.y..r.y + r.height).filter(|&y| same(b, y)).count() as u32 * 2 >= r.height).collect();
        if moving.is_empty() { return None; }
        let (mut best, mut run) = ((0, 0), None::<u32>);
        for y in r.y..=r.y + r.height {
            let all = y < r.y + r.height && moving.iter().all(|&b| same(b, y));
            match (all, run) {
                (true, None) => run = Some(y),
                (false, Some(start)) => { if y - start > best.1 { best = (start, y - start); } run = None; }
                _ => {}
            }
        }
        let (y0, height) = best;
        if height < MIN_ROWS { return None; }
        let groups = |members: &dyn Fn(u32) -> bool| {
            let mut spans: Vec<(u32, u32)> = Vec::new();
            for band in (first_band..=last_band).filter(|&b| members(b)) {
                let (x0, x1) = columns(band);
                match spans.last_mut() { Some(last) if last.1 == x0 => last.1 = x1, _ => spans.push((x0, x1)) }
            }
            spans
        };
        let moves: Vec<Move> = groups(&|b| moving.contains(&b)).into_iter()
            .map(|(x0, x1)| Move { x: x0, width: x1 - x0, src_y: (i64::from(y0) + dy) as u32, dst_y: y0, height })
            .collect();
        let mut pieces = Vec::new();
        if y0 > r.y { pieces.push(Rect { x: r.x, y: r.y, width: r.width, height: y0 - r.y }); }
        if y0 + height < r.y + r.height { pieces.push(Rect { x: r.x, y: y0 + height, width: r.width, height: r.y + r.height - y0 - height }); }
        for (x0, x1) in groups(&|b| !moving.contains(&b)) { pieces.push(Rect { x: x0, y: y0, width: x1 - x0, height }); }
        (pieces.len() <= MAX_PIECES).then_some((moves, pieces))
    }

    /// Notes that the host now holds `current` inside `rects`.
    pub fn record(&mut self, current: &[u8], rects: &[Rect]) {
        let bands = self.bands();
        for r in rects {
            for y in r.y..r.y + r.height {
                let row = self.row(y, r.x, r.x + r.width);
                self.pixels[row.clone()].copy_from_slice(&current[row]);
            }
            for y in r.y..r.y + r.height {
                for band in r.x / BAND..=(r.x + r.width - 1) / BAND {
                    let at = (y * bands + band) as usize;
                    match self.fresh[at].take() { Some(h) => { self.hashes[at] = h; self.stale[at] = false; } None => self.stale[at] = true }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rebuild(size: Size, shipped: &[u8], current: &[u8], moves: &[Move], rects: &[Rect]) -> Vec<u8> {
        let mut out = shipped.to_vec();
        for m in moves { m.apply(size, &mut out); }
        let stride = size.width() as usize * 4;
        for r in rects {
            for y in r.y..r.y + r.height {
                let a = y as usize * stride + r.x as usize * 4;
                out[a..a + r.width as usize * 4].copy_from_slice(&current[a..a + r.width as usize * 4]);
            }
        }
        out
    }

    fn page(size: Size, seed: u32) -> Vec<u8> {
        (0..size.pixel_len() as u32).map(|i| { let (p, y) = (i / 4, i / 4 / size.width()); ((p.wrapping_mul(2_654_435_761) >> 13) ^ (y.wrapping_add(seed) * 31)) as u8 }).collect()
    }

    #[test]
    fn a_scroll_becomes_a_move_beside_a_scrollbar_and_the_exposed_rows() {
        let size = Size::new(300, 240).unwrap();
        let tall = page(Size::new(300, 400).unwrap(), 0);
        let stride = 300 * 4;
        let before = tall[..240 * stride].to_vec();
        let mut after = tall[40 * stride..280 * stride].to_vec();
        for y in 0..240 { for x in 290..300 { after[y * stride + x * 4..][..4].copy_from_slice(&[y as u8, 1, 2, 0]); } }
        let mut shipped = Shipped::new(size);
        let whole = [Rect { x: 0, y: 0, width: 300, height: 240 }];
        shipped.record(&before, &whole);
        let (moves, rects) = shipped.plan(&after, &whole);
        assert_eq!(moves.len(), 1);
        assert_eq!((moves[0].src_y, moves[0].dst_y, moves[0].height, moves[0].x), (40, 0, 200, 0));
        assert!(moves[0].width < 300, "the scrollbar's band does not move");
        let sent: u32 = rects.iter().map(|r| r.width * r.height).sum();
        assert!(sent < 300 * 240 / 3);
        assert_eq!(rebuild(size, &before, &after, &moves, &rects), after);
    }

    #[test]
    fn flat_blocks_with_short_labels_still_scroll_as_a_move() {
        let size = Size::new(320, 400).unwrap();
        let block = |y: u32, x: u32| -> [u8; 4] {
            let (i, within) = (y / 40, y % 40);
            if x < 60 && (10..30).contains(&within) && (x * 7 + within * 3 + i) % 5 == 0 { [0, 0, 0, 0] } else { [(i * 7) as u8, (i * 13) as u8, 200, 0] }
        };
        let render = |top: u32| -> Vec<u8> { (0..400).flat_map(|y| (0..320).flat_map(move |x| block(y + top, x))).collect() };
        let (before, after) = (render(0), render(37));
        let whole = [Rect { x: 0, y: 0, width: 320, height: 400 }];
        let mut shipped = Shipped::new(size);
        shipped.record(&before, &whole);
        let (moves, rects) = shipped.plan(&after, &whole);
        assert!(!moves.is_empty(), "the scrolled blocks move");
        assert!(rects.iter().map(|r| r.width * r.height).sum::<u32>() < 320 * 400 / 4);
        assert_eq!(rebuild(size, &before, &after, &moves, &rects), after);
    }

    #[test]
    fn planning_backs_off_after_misses_and_finds_scrolls_again() {
        let size = Size::new(200, 300).unwrap();
        let tall = page(Size::new(200, 900).unwrap(), 5);
        let stride = 200 * 4;
        let whole = [Rect { x: 0, y: 0, width: 200, height: 300 }];
        let mut shipped = Shipped::new(size);
        let mut host = vec![0; size.pixel_len()];
        let mut found = Vec::new();
        for frame in 0..30usize {
            let current = if frame < 10 { page(size, frame as u32 + 20) } else { tall[(frame - 10) * 11 * stride..][..300 * stride].to_vec() };
            let (moves, sent) = shipped.plan(&current, &whole);
            found.push(!moves.is_empty());
            host = rebuild(size, &host, &current, &moves, &sent);
            assert_eq!(host, current, "frame {frame}");
            shipped.record(&current, &whole);
        }
        assert!(found[..10].iter().all(|f| !f), "unrelated frames have no moves");
        assert!(found[20..].iter().filter(|f| **f).count() >= 8, "a sustained scroll is found again after backing off");
    }

    #[test]
    fn unchanged_or_unrelated_damage_has_no_moves() {
        let size = Size::new(128, 128).unwrap();
        let whole = [Rect { x: 0, y: 0, width: 128, height: 128 }];
        let mut shipped = Shipped::new(size);
        let first = page(size, 1);
        shipped.record(&first, &whole);
        assert_eq!(shipped.plan(&first, &whole), (vec![], whole.to_vec()));
        assert_eq!(shipped.plan(&page(size, 9), &whole).0, vec![]);
        let blank = vec![0xff; size.pixel_len()];
        shipped.record(&blank, &whole);
        assert_eq!(shipped.plan(&blank, &whole).0, vec![]);
    }

    #[test]
    fn plans_always_rebuild_the_current_frame() {
        let size = Size::new(200, 160).unwrap();
        let mut seed = 7u32;
        let mut next = || { seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345); seed >> 8 };
        let tall = page(Size::new(200, 480).unwrap(), 3);
        let stride = 200 * 4;
        let mut shipped = Shipped::new(size);
        let mut host = vec![0; size.pixel_len()];
        for _ in 0..60 {
            let offset = (next() % 300) as usize;
            let mut current = tall[offset * stride..(offset + 160) * stride].to_vec();
            if next() % 2 == 0 { for b in &mut current[(next() % 150) as usize * stride..][..stride * 4] { *b = 0x55; } }
            let rects: Vec<Rect> = match next() % 4 {
                0 => vec![Rect { x: 0, y: 0, width: 200, height: 160 }],
                1 => vec![Rect { x: 10, y: 0, width: 150, height: 160 }, Rect { x: 0, y: 0, width: 10, height: 160 }, Rect { x: 160, y: 0, width: 40, height: 160 }],
                2 => vec![Rect { x: 0, y: 0, width: 200, height: 100 }, Rect { x: 0, y: 100, width: 200, height: 60 }],
                _ => (0..5).map(|i| Rect { x: 0, y: i * 32, width: 200, height: 32 }).chain([Rect { x: 0, y: 0, width: 1, height: 1 }].into_iter().filter(|_| false)).collect(),
            };
            let (moves, sent) = shipped.plan(&current, &rects);
            host = rebuild(size, &host, &current, &moves, &sent);
            assert_eq!(host, current);
            shipped.record(&current, &rects);
        }
    }
}
