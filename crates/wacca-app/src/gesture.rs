//! Menu pointer gestures: a press that barely moves is a tap; otherwise it drags (scrolls).

/// Movement (px) beyond which a press becomes a drag.
pub const TAP_SLOP: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gesture {
    Tap([f32; 2]),
    /// Vertical movement since the previous drag event (px, down positive).
    Drag(f32),
}

#[derive(Clone, Copy, Debug)]
struct Press {
    start: [f32; 2],
    last_y: f32,
    dragging: bool,
}

/// One pointer (mouse or a touch id).
#[derive(Clone, Copy, Debug, Default)]
pub struct Pointer {
    press: Option<Press>,
}

impl Pointer {
    pub fn down(&mut self, p: [f32; 2]) {
        self.press = Some(Press {
            start: p,
            last_y: p[1],
            dragging: false,
        });
    }

    pub fn moved(&mut self, p: [f32; 2]) -> Option<Gesture> {
        let press = self.press.as_mut()?;
        let d = (p[0] - press.start[0]).hypot(p[1] - press.start[1]);
        press.dragging |= d > TAP_SLOP;
        if !press.dragging {
            return None;
        }
        let dy = p[1] - press.last_y;
        press.last_y = p[1];
        Some(Gesture::Drag(dy))
    }

    pub fn up(&mut self, p: [f32; 2]) -> Option<Gesture> {
        let press = self.press.take()?;
        let d = (p[0] - press.start[0]).hypot(p[1] - press.start[1]);
        (!press.dragging && d <= TAP_SLOP).then_some(Gesture::Tap(press.start))
    }
}

/// Turns drag distance into whole list steps of `step` px (dragging down scrolls up the list).
#[derive(Clone, Copy, Debug, Default)]
pub struct Scroller {
    acc: f32,
}

impl Scroller {
    pub fn steps(&mut self, dy: f32, step: f32) -> i32 {
        if step <= 0.0 {
            return 0;
        }
        self.acc += dy;
        let n = (self.acc / step).trunc();
        self.acc -= n * step;
        -(n as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_movement_is_a_tap() {
        let mut p = Pointer::default();
        p.down([10.0, 10.0]);
        assert_eq!(p.moved([14.0, 13.0]), None);
        assert_eq!(p.up([14.0, 13.0]), Some(Gesture::Tap([10.0, 10.0])));
        assert_eq!(p.up([0.0, 0.0]), None);
        assert_eq!(p.moved([0.0, 0.0]), None);
    }

    #[test]
    fn large_movement_drags_and_never_taps() {
        let mut p = Pointer::default();
        p.down([10.0, 10.0]);
        assert_eq!(p.moved([10.0, 30.0]), Some(Gesture::Drag(20.0)));
        assert_eq!(p.moved([10.0, 25.0]), Some(Gesture::Drag(-5.0)));
        assert_eq!(p.moved([10.0, 12.0]), Some(Gesture::Drag(-13.0)));
        assert_eq!(p.up([10.0, 10.0]), None);
    }

    #[test]
    fn scroller_accumulates_whole_steps() {
        let mut s = Scroller::default();
        assert_eq!(s.steps(15.0, 20.0), 0);
        assert_eq!(s.steps(10.0, 20.0), -1);
        assert_eq!(s.steps(-50.0, 20.0), 2);
        assert_eq!(s.steps(5.0, 0.0), 0);
    }
}
