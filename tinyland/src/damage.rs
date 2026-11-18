use crate::positioner::Rect;

/// Rectangles past which damage is tracked as the whole area.
const MAX_RECTS: usize = 32;

/// Changed pixels since some earlier state, in one coordinate space. Bounded: too many rectangles become `full`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Damage {
    rects: Vec<Rect>,
    full: bool,
}

impl Damage {
    pub fn full() -> Self { Self { rects: Vec::new(), full: true } }

    pub fn is_full(&self) -> bool { self.full }

    pub fn rects(&self) -> &[Rect] { &self.rects }

    /// Adds a rectangle; empty and negative-size rectangles change nothing.
    pub fn add(&mut self, rect: Rect) {
        if self.full || rect.width <= 0 || rect.height <= 0 { return; }
        if self.rects.len() == MAX_RECTS { *self = Self::full(); return; }
        self.rects.push(rect);
    }

    pub fn merge(&mut self, other: &Damage) {
        if other.full { *self = Self::full(); return; }
        for rect in &other.rects { self.add(*rect); }
    }

    pub fn clear(&mut self) { *self = Self::default(); }
}

/// The part of `rect` inside a `width` x `height` area, as `(x0, y0, x1, y1)`.
pub fn clip(rect: Rect, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let x0 = i64::from(rect.x).max(0);
    let y0 = i64::from(rect.y).max(0);
    let x1 = (i64::from(rect.x) + i64::from(rect.width)).min(i64::from(width));
    let y1 = (i64::from(rect.y) + i64::from(rect.height)).min(i64::from(height));
    (x0 < x1 && y0 < y1).then(|| (x0 as u32, y0 as u32, x1 as u32, y1 as u32))
}

/// `damage` clipped to a `width` x `height` area, with overlapping rectangles united, and at most `limit`
/// rectangles: past that, their bounding box. Full damage is the whole area.
pub fn output_rects(damage: &Damage, width: u32, height: u32, limit: usize) -> Vec<(u32, u32, u32, u32)> {
    if damage.full { return vec![(0, 0, width, height)]; }
    let mut rects: Vec<_> = damage.rects.iter().filter_map(|rect| clip(*rect, width, height)).collect();
    let mut merged = true;
    while merged {
        merged = false;
        'outer: for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                let (a, b) = (rects[i], rects[j]);
                if a.0 <= b.2 && b.0 <= a.2 && a.1 <= b.3 && b.1 <= a.3 {
                    rects[i] = (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3));
                    rects.swap_remove(j);
                    merged = true;
                    break 'outer;
                }
            }
        }
    }
    if rects.len() > limit {
        let bound = rects.iter().fold(rects[0], |a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)));
        rects = vec![bound];
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect { Rect { x, y, width, height } }

    #[test]
    fn damage_is_bounded_and_full_absorbs_everything() {
        let mut damage = Damage::default();
        damage.add(rect(0, 0, 0, 5));
        damage.add(rect(0, 0, -3, 5));
        assert_eq!(damage, Damage::default());
        for n in 0..MAX_RECTS as i32 { damage.add(rect(n, 0, 1, 1)); }
        assert!(!damage.is_full());
        damage.add(rect(99, 0, 1, 1));
        assert!(damage.is_full());
        let mut other = Damage::default();
        other.add(rect(1, 1, 1, 1));
        other.merge(&Damage::full());
        assert!(other.is_full() && other.rects().is_empty());
    }

    #[test]
    fn output_rects_clip_unite_overlaps_and_fall_back_to_a_bound() {
        let mut damage = Damage::default();
        damage.add(rect(-10, -10, 15, 15));
        damage.add(rect(3, 3, 4, 4));
        damage.add(rect(50, 50, 10, 10));
        damage.add(rect(i32::MAX, 0, i32::MAX, 1));
        damage.add(rect(90, 0, 100, 2));
        let mut got = output_rects(&damage, 100, 60, 8);
        got.sort();
        assert_eq!(got, vec![(0, 0, 7, 7), (50, 50, 60, 60), (90, 0, 100, 2)]);
        assert_eq!(output_rects(&damage, 100, 60, 2), vec![(0, 0, 100, 60)]);
        assert_eq!(output_rects(&Damage::full(), 30, 20, 8), vec![(0, 0, 30, 20)]);
        assert_eq!(clip(rect(i32::MIN, i32::MIN, i32::MAX, i32::MAX), 10, 10), None);
    }
}
