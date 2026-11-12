use wayland_protocols::xdg::shell::server::xdg_positioner::{Anchor, ConstraintAdjustment, Gravity};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= f64::from(self.x)
            && y >= f64::from(self.y)
            && x < f64::from(self.x) + f64::from(self.width)
            && y < f64::from(self.y) + f64::from(self.height)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

impl From<Rect> for Bounds {
    fn from(rect: Rect) -> Self {
        Bounds { x: rect.x.into(), y: rect.y.into(), width: rect.width.into(), height: rect.height.into() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unrepresentable;

pub fn clip_to_i32(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Start,
    Center,
    End,
}

impl Side {
    fn flipped(self) -> Self {
        match self {
            Side::Start => Side::End,
            Side::Center => Side::Center,
            Side::End => Side::Start,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sides {
    x: Side,
    y: Side,
}

fn anchor_sides(anchor: Anchor) -> Sides {
    let (x, y) = match anchor {
        Anchor::Top => (Side::Center, Side::Start),
        Anchor::Bottom => (Side::Center, Side::End),
        Anchor::Left => (Side::Start, Side::Center),
        Anchor::Right => (Side::End, Side::Center),
        Anchor::TopLeft => (Side::Start, Side::Start),
        Anchor::BottomLeft => (Side::Start, Side::End),
        Anchor::TopRight => (Side::End, Side::Start),
        Anchor::BottomRight => (Side::End, Side::End),
        _ => (Side::Center, Side::Center),
    };
    Sides { x, y }
}

fn gravity_sides(gravity: Gravity) -> Sides {
    let (x, y) = match gravity {
        Gravity::Top => (Side::Center, Side::Start),
        Gravity::Bottom => (Side::Center, Side::End),
        Gravity::Left => (Side::Start, Side::Center),
        Gravity::Right => (Side::End, Side::Center),
        Gravity::TopLeft => (Side::Start, Side::Start),
        Gravity::BottomLeft => (Side::Start, Side::End),
        Gravity::TopRight => (Side::End, Side::Start),
        Gravity::BottomRight => (Side::End, Side::End),
        _ => (Side::Center, Side::Center),
    };
    Sides { x, y }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Positioner {
    pub size: Option<(i32, i32)>,
    pub anchor_rect: Option<Rect>,
    pub anchor: Anchor,
    pub gravity: Gravity,
    pub constraint_adjustment: ConstraintAdjustment,
    pub offset: (i32, i32),
}

impl Default for Positioner {
    fn default() -> Self {
        Positioner {
            size: None,
            anchor_rect: None,
            anchor: Anchor::None,
            gravity: Gravity::None,
            constraint_adjustment: ConstraintAdjustment::empty(),
            offset: (0, 0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Complete {
    size: (i32, i32),
    anchor_rect: Rect,
    anchor: Sides,
    gravity: Sides,
    adjustment: ConstraintAdjustment,
    offset: (i32, i32),
}

impl Positioner {
    pub fn complete(&self) -> Option<Complete> {
        Some(Complete {
            size: self.size?,
            anchor_rect: self.anchor_rect?,
            anchor: anchor_sides(self.anchor),
            gravity: gravity_sides(self.gravity),
            adjustment: self.constraint_adjustment,
            offset: self.offset,
        })
    }
}

fn anchor_point(start: i64, length: i64, side: Side) -> i64 {
    match side {
        Side::Start => start,
        Side::Center => start + length / 2,
        Side::End => start + length,
    }
}

fn place_along(point: i64, length: i64, gravity: Side) -> i64 {
    match gravity {
        Side::Start => point - length,
        Side::Center => point - length / 2,
        Side::End => point,
    }
}

struct Axis {
    anchor_start: i64,
    anchor_length: i64,
    anchor: Side,
    gravity: Side,
    offset: i64,
    length: i64,
    bounds_start: i64,
    bounds_length: i64,
    flip: bool,
    slide: bool,
    resize: bool,
}

impl Axis {
    fn position(&self, anchor: Side, gravity: Side, offset: i64) -> i64 {
        place_along(anchor_point(self.anchor_start, self.anchor_length, anchor), self.length, gravity) + offset
    }

    fn fits(&self, start: i64, length: i64) -> bool {
        start >= self.bounds_start && start + length <= self.bounds_start + self.bounds_length
    }

    fn solve(&self) -> (i64, i64) {
        let mut start = self.position(self.anchor, self.gravity, self.offset);
        let mut length = self.length;
        if self.fits(start, length) {
            return (start, length);
        }
        if self.flip {
            let flipped = self.position(self.anchor.flipped(), self.gravity.flipped(), -self.offset);
            if self.fits(flipped, length) {
                return (flipped, length);
            }
        }
        if self.slide {
            let end = self.bounds_start + self.bounds_length;
            if start + length > end {
                start = end - length;
            }
            if start < self.bounds_start {
                start = self.bounds_start;
            }
            if self.fits(start, length) {
                return (start, length);
            }
        }
        if self.resize {
            let clipped_start = start.max(self.bounds_start);
            let clipped_end = (start + length).min(self.bounds_start + self.bounds_length);
            if clipped_end > clipped_start {
                start = clipped_start;
                length = clipped_end - clipped_start;
            }
        }
        (start, length)
    }
}

impl Complete {
    pub fn place(&self, bounds: impl Into<Bounds>) -> Result<Rect, Unrepresentable> {
        let bounds = bounds.into();
        let adjust = self.adjustment;
        let x = Axis {
            anchor_start: self.anchor_rect.x.into(),
            anchor_length: self.anchor_rect.width.into(),
            anchor: self.anchor.x,
            gravity: self.gravity.x,
            offset: self.offset.0.into(),
            length: self.size.0.into(),
            bounds_start: bounds.x,
            bounds_length: bounds.width,
            flip: adjust.contains(ConstraintAdjustment::FlipX),
            slide: adjust.contains(ConstraintAdjustment::SlideX),
            resize: adjust.contains(ConstraintAdjustment::ResizeX),
        }
        .solve();
        let y = Axis {
            anchor_start: self.anchor_rect.y.into(),
            anchor_length: self.anchor_rect.height.into(),
            anchor: self.anchor.y,
            gravity: self.gravity.y,
            offset: self.offset.1.into(),
            length: self.size.1.into(),
            bounds_start: bounds.y,
            bounds_length: bounds.height,
            flip: adjust.contains(ConstraintAdjustment::FlipY),
            slide: adjust.contains(ConstraintAdjustment::SlideY),
            resize: adjust.contains(ConstraintAdjustment::ResizeY),
        }
        .solve();
        let narrow = |value: i64| i32::try_from(value).map_err(|_| Unrepresentable);
        Ok(Rect { x: narrow(x.0)?, y: narrow(y.0)?, width: narrow(x.1)?, height: narrow(y.1)? })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT: Rect = Rect { x: 0, y: 0, width: 1280, height: 800 };

    fn select_menu(anchor_y: i32, height: i32, adjustment: ConstraintAdjustment) -> Complete {
        Positioner {
            size: Some((320, height)),
            anchor_rect: Some(Rect { x: 40, y: anchor_y, width: 320, height: 36 }),
            anchor: Anchor::BottomLeft,
            gravity: Gravity::BottomRight,
            constraint_adjustment: adjustment,
            offset: (0, 0),
        }
        .complete()
        .unwrap()
    }

    #[test]
    fn incomplete_positioner_is_rejected() {
        let mut positioner = Positioner { size: Some((10, 10)), ..Positioner::default() };
        assert!(positioner.complete().is_none());
        positioner.size = None;
        positioner.anchor_rect = Some(Rect { x: 0, y: 0, width: 1, height: 1 });
        assert!(positioner.complete().is_none());
    }

    #[test]
    fn unconstrained_popup_hangs_from_its_anchor_edge() {
        let placed = select_menu(88, 120, ConstraintAdjustment::empty()).place(OUTPUT).unwrap();
        assert_eq!(placed, Rect { x: 40, y: 124, width: 320, height: 120 });
    }

    #[test]
    fn centered_anchor_and_gravity_center_the_popup_on_the_rect() {
        let placed = Positioner {
            size: Some((100, 50)),
            anchor_rect: Some(Rect { x: 200, y: 200, width: 40, height: 20 }),
            ..Positioner::default()
        }
        .complete()
        .unwrap()
        .place(OUTPUT).unwrap();
        assert_eq!(placed, Rect { x: 170, y: 185, width: 100, height: 50 });
    }

    #[test]
    fn offset_shifts_the_unconstrained_position() {
        let mut positioner = Positioner {
            size: Some((50, 50)),
            anchor_rect: Some(Rect { x: 10, y: 10, width: 10, height: 10 }),
            anchor: Anchor::BottomRight,
            gravity: Gravity::BottomRight,
            ..Positioner::default()
        };
        positioner.offset = (3, -4);
        assert_eq!(positioner.complete().unwrap().place(OUTPUT).unwrap(), Rect { x: 23, y: 16, width: 50, height: 50 });
    }

    #[test]
    fn flip_moves_a_menu_above_its_anchor_when_below_overflows() {
        let adjust = ConstraintAdjustment::FlipY | ConstraintAdjustment::SlideY;
        let placed = select_menu(700, 200, adjust).place(OUTPUT).unwrap();
        assert_eq!(placed, Rect { x: 40, y: 500, width: 320, height: 200 });
    }

    #[test]
    fn flip_is_reverted_when_the_flipped_position_also_overflows_and_slide_takes_over() {
        let adjust = ConstraintAdjustment::FlipY | ConstraintAdjustment::SlideY;
        let placed = select_menu(300, 600, adjust).place(OUTPUT).unwrap();
        assert_eq!(placed, Rect { x: 40, y: 200, width: 320, height: 600 });
    }

    #[test]
    fn slide_aligns_to_the_start_edge_when_larger_than_the_output() {
        let placed = select_menu(10, 1000, ConstraintAdjustment::SlideY).place(OUTPUT).unwrap();
        assert_eq!(placed.y, 0);
        assert_eq!(placed.height, 1000);
    }

    #[test]
    fn resize_clips_to_the_output_when_nothing_else_is_allowed() {
        let placed = select_menu(700, 200, ConstraintAdjustment::ResizeY).place(OUTPUT).unwrap();
        assert_eq!(placed, Rect { x: 40, y: 736, width: 320, height: 64 });
    }

    #[test]
    fn no_adjustment_leaves_the_popup_constrained() {
        let placed = select_menu(700, 200, ConstraintAdjustment::empty()).place(OUTPUT).unwrap();
        assert_eq!(placed.y, 736);
        assert_eq!(placed.height, 200);
    }

    #[test]
    fn flip_x_mirrors_a_submenu_to_the_left_of_its_parent_item() {
        let placed = Positioner {
            size: Some((200, 100)),
            anchor_rect: Some(Rect { x: 1100, y: 100, width: 150, height: 24 }),
            anchor: Anchor::TopRight,
            gravity: Gravity::BottomRight,
            constraint_adjustment: ConstraintAdjustment::FlipX,
            offset: (0, 0),
        }
        .complete()
        .unwrap()
        .place(OUTPUT).unwrap();
        assert_eq!(placed, Rect { x: 900, y: 100, width: 200, height: 100 });
    }

    #[test]
    fn bounds_are_relative_to_the_parent_geometry() {
        let bounds = Rect { x: -500, y: -300, width: 1280, height: 800 };
        let adjust = ConstraintAdjustment::FlipY;
        let placed = select_menu(400, 200, adjust).place(bounds).unwrap();
        assert_eq!(placed.y, 200);
    }

    const SIDES: [Anchor; 9] = [
        Anchor::None, Anchor::Top, Anchor::Bottom, Anchor::Left, Anchor::Right,
        Anchor::TopLeft, Anchor::BottomLeft, Anchor::TopRight, Anchor::BottomRight,
    ];

    const GRAVITIES: [Gravity; 9] = [
        Gravity::None, Gravity::Top, Gravity::Bottom, Gravity::Left, Gravity::Right,
        Gravity::TopLeft, Gravity::BottomLeft, Gravity::TopRight, Gravity::BottomRight,
    ];

    const EXTREMES: [i32; 7] = [i32::MIN, i32::MIN + 1, -1, 0, 1, i32::MAX - 1, i32::MAX];

    const BOUNDS: Rect = Rect { x: 0, y: 0, width: 100, height: 100 };

    fn placed(complete: Complete, bounds: Rect) -> Option<Rect> {
        complete.place(bounds).ok()
    }

    fn ticket_offset(adjustment: ConstraintAdjustment) -> Complete {
        Positioner {
            size: Some((8, 8)),
            anchor_rect: Some(Rect { x: 40, y: 40, width: 4, height: 4 }),
            constraint_adjustment: adjustment,
            offset: (i32::MAX, 0),
            ..Positioner::default()
        }
        .complete()
        .unwrap()
    }

    #[test]
    fn offset_past_i32_without_adjustment_is_unrepresentable() {
        assert_eq!(placed(ticket_offset(ConstraintAdjustment::empty()), BOUNDS), None);
        assert_eq!(placed(ticket_offset(ConstraintAdjustment::FlipX), BOUNDS), None);
    }

    #[test]
    fn slide_resolves_an_offset_past_i32_from_the_exact_position() {
        let placed = placed(ticket_offset(ConstraintAdjustment::SlideX), BOUNDS).unwrap();
        assert_eq!(placed, Rect { x: 92, y: 38, width: 8, height: 8 });
    }

    #[test]
    fn resize_clips_an_offset_past_i32_to_nothing_and_stays_unrepresentable() {
        assert_eq!(placed(ticket_offset(ConstraintAdjustment::ResizeX), BOUNDS), None);
    }

    #[test]
    fn most_negative_offset_with_flip_keeps_the_original_side_when_neither_fits() {
        let complete = Positioner {
            size: Some((2, 2)),
            anchor_rect: Some(Rect { x: 0, y: 0, width: 2, height: 2 }),
            constraint_adjustment: ConstraintAdjustment::FlipX,
            offset: (i32::MIN, 0),
            ..Positioner::default()
        }
        .complete()
        .unwrap();
        assert_eq!(placed(complete, BOUNDS), Some(Rect { x: i32::MIN, y: 0, width: 2, height: 2 }));
    }

    #[test]
    fn positions_that_only_wrap_into_bounds_do_not_suppress_a_flip() {
        let complete = Positioner {
            size: Some((8, 8)),
            anchor_rect: Some(Rect { x: i32::MIN, y: 0, width: 20, height: 10 }),
            anchor: Anchor::Left,
            gravity: Gravity::Right,
            constraint_adjustment: ConstraintAdjustment::FlipX,
            offset: (i32::MIN, 0),
        }
        .complete()
        .unwrap();
        assert_eq!(placed(complete, BOUNDS), Some(Rect { x: 12, y: 1, width: 8, height: 8 }));
    }

    #[test]
    fn bounds_wider_than_i32_still_decide_adjustments() {
        let complete = select_menu(0, 50, ConstraintAdjustment::SlideY);
        let bounds = Bounds { x: 0, y: 5_000_000_000, width: 100, height: 100 };
        assert_eq!(complete.place(bounds), Err(Unrepresentable));
        let bounds = Bounds { x: 0, y: i64::from(i32::MAX) - 49, width: 100, height: 100 };
        assert_eq!(complete.place(bounds).unwrap().y, i32::MAX - 49);
    }

    fn unconstrained(rect: Rect, size: (i32, i32), offset: (i32, i32)) -> Complete {
        Positioner { size: Some(size), anchor_rect: Some(rect), offset, ..Positioner::default() }.complete().unwrap()
    }

    fn exact(rect_start: i32, rect_length: i32, length: i32, offset: i32) -> i128 {
        i128::from(rect_start) + i128::from(rect_length) / 2 - i128::from(length) / 2 + i128::from(offset)
    }

    #[test]
    fn unconstrained_placement_is_exact_or_unrepresentable_over_extreme_inputs() {
        for start in EXTREMES {
            for rect_length in [0, 1, i32::MAX] {
                for length in [1, 2, i32::MAX] {
                    for offset in EXTREMES {
                        let want_x = exact(start, rect_length, length, offset);
                        let across = unconstrained(Rect { x: start, y: 0, width: rect_length, height: 2 }, (length, 2), (offset, 0));
                        let want_y = exact(start, rect_length, length, offset);
                        let down = unconstrained(Rect { x: 0, y: start, width: 2, height: rect_length }, (2, length), (0, offset));
                        match i32::try_from(want_x) {
                            Ok(x) => assert_eq!(placed(across, BOUNDS), Some(Rect { x, y: 0, width: length, height: 2 })),
                            Err(_) => assert_eq!(placed(across, BOUNDS), None, "x {want_x}"),
                        }
                        match i32::try_from(want_y) {
                            Ok(y) => assert_eq!(placed(down, BOUNDS), Some(Rect { x: 0, y, width: 2, height: length })),
                            Err(_) => assert_eq!(placed(down, BOUNDS), None, "y {want_y}"),
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn every_anchor_gravity_and_adjustment_survives_extreme_inputs() {
        let adjustments = [
            ConstraintAdjustment::empty(),
            ConstraintAdjustment::FlipX | ConstraintAdjustment::FlipY,
            ConstraintAdjustment::SlideX | ConstraintAdjustment::SlideY,
            ConstraintAdjustment::ResizeX | ConstraintAdjustment::ResizeY,
            ConstraintAdjustment::all(),
        ];
        for anchor in SIDES {
            for gravity in GRAVITIES {
                for adjustment in adjustments {
                    for start in EXTREMES {
                        for offset in EXTREMES {
                            let complete = Positioner {
                                size: Some((i32::MAX, 3)),
                                anchor_rect: Some(Rect { x: start, y: start, width: i32::MAX, height: i32::MAX }),
                                anchor,
                                gravity,
                                constraint_adjustment: adjustment,
                                offset: (offset, offset),
                            }
                            .complete()
                            .unwrap();
                            if let Some(rect) = placed(complete, BOUNDS) {
                                assert!(rect.width >= 1 && rect.height >= 1 && rect.height <= 3, "{rect:?}");
                            }
                        }
                    }
                }
            }
        }
    }
}
