//! Controller support for handhelds: a background thread reads any connected gamepad (gilrs: evdev on
//! Linux, XInput on Windows) and sends simple navigation actions to the UI thread.
//! The app turns them into the key presses egui already understands (arrows move focus, Enter clicks).
use eframe::egui;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum Pad {
    Up,
    Down,
    Left,
    Right,
    /// A / Cross: activate the focused control.
    Accept,
    /// B / Circle: close a popup or leave a text field.
    Back,
    /// LB / RB: previous / next tab.
    PrevTab,
    NextTab,
    /// Start / Menu: PLAY.
    Start,
    /// Right stick: scroll, in points (positive = down).
    Scroll(f32),
    /// A controller was connected (its name).
    Connected(String),
}

const DEAD_ZONE: f32 = 0.55;
const SCROLL_DEAD_ZONE: f32 = 0.2;
const FIRST_REPEAT: Duration = Duration::from_millis(400);
const REPEAT: Duration = Duration::from_millis(120);

/// Starts reading controllers. Returns None when the system offers no gamepad support.
pub fn start(ctx: egui::Context) -> Option<Receiver<Pad>> {
    let (tx, rx) = channel();
    std::thread::Builder::new().name("gamepad".into()).spawn(move || run(tx, ctx)).ok()?;
    Some(rx)
}

fn run(tx: Sender<Pad>, ctx: egui::Context) {
    use gilrs::{Axis, Button, EventType, Gilrs};
    let Ok(mut gilrs) = Gilrs::new() else { return };
    let send = |p: Pad| {
        let ok = tx.send(p).is_ok();
        ctx.request_repaint();
        ok
    };
    for (_, g) in gilrs.gamepads() {
        send(Pad::Connected(g.name().to_string()));
    }
    // the direction held on the D-pad or left stick, and when it should repeat next
    let mut held: Option<(Pad, Instant)> = None;
    let mut dpad: Option<Pad> = None;
    let mut stick = (0.0f32, 0.0f32);
    let mut scroll = 0.0f32;
    loop {
        while let Some(ev) = gilrs.next_event_blocking(Some(Duration::from_millis(16))) {
            let action = match ev.event {
                EventType::Connected => Some(Pad::Connected(gilrs.gamepad(ev.id).name().to_string())),
                EventType::ButtonPressed(b, _) => match b {
                    Button::DPadUp => {
                        dpad = Some(Pad::Up);
                        None
                    }
                    Button::DPadDown => {
                        dpad = Some(Pad::Down);
                        None
                    }
                    Button::DPadLeft => {
                        dpad = Some(Pad::Left);
                        None
                    }
                    Button::DPadRight => {
                        dpad = Some(Pad::Right);
                        None
                    }
                    Button::South => Some(Pad::Accept),
                    Button::East => Some(Pad::Back),
                    Button::LeftTrigger => Some(Pad::PrevTab),
                    Button::RightTrigger => Some(Pad::NextTab),
                    Button::Start => Some(Pad::Start),
                    _ => None,
                },
                EventType::ButtonReleased(b, _) => {
                    if matches!(b, Button::DPadUp | Button::DPadDown | Button::DPadLeft | Button::DPadRight) {
                        dpad = None;
                    }
                    None
                }
                EventType::AxisChanged(Axis::LeftStickX, v, _) => {
                    stick.0 = v;
                    None
                }
                EventType::AxisChanged(Axis::LeftStickY, v, _) => {
                    stick.1 = v;
                    None
                }
                EventType::AxisChanged(Axis::RightStickY, v, _) => {
                    scroll = v;
                    None
                }
                _ => None,
            };
            if let Some(a) = action {
                if !send(a) {
                    return;
                }
            }
        }
        // one direction at a time: the D-pad wins over the stick
        let dir = dpad.clone().or_else(|| stick_direction(stick.0, stick.1));
        let now = Instant::now();
        match (dir, held.clone()) {
            (Some(d), Some((h, next))) if d == h => {
                if now >= next {
                    held = Some((d.clone(), now + REPEAT));
                    if !send(d) {
                        return;
                    }
                }
            }
            (Some(d), _) => {
                held = Some((d.clone(), now + FIRST_REPEAT));
                if !send(d) {
                    return;
                }
            }
            (None, _) => held = None,
        }
        if scroll.abs() > SCROLL_DEAD_ZONE && !send(Pad::Scroll(-scroll * 18.0)) {
            return;
        }
    }
}

/// gilrs reports Y up as positive.
fn stick_direction(x: f32, y: f32) -> Option<Pad> {
    if x.abs() < DEAD_ZONE && y.abs() < DEAD_ZONE {
        None
    } else if x.abs() > y.abs() {
        Some(if x > 0.0 { Pad::Right } else { Pad::Left })
    } else {
        Some(if y > 0.0 { Pad::Up } else { Pad::Down })
    }
}

/// The key events egui understands for a direction.
pub fn key_for(p: &Pad) -> Option<egui::Key> {
    Some(match p {
        Pad::Up => egui::Key::ArrowUp,
        Pad::Down => egui::Key::ArrowDown,
        Pad::Left => egui::Key::ArrowLeft,
        Pad::Right => egui::Key::ArrowRight,
        Pad::Accept => egui::Key::Enter,
        Pad::Back => egui::Key::Escape,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stick_picks_the_stronger_axis_outside_the_dead_zone() {
        assert_eq!(stick_direction(0.1, -0.2), None);
        assert_eq!(stick_direction(0.9, 0.3), Some(Pad::Right));
        assert_eq!(stick_direction(-0.2, 0.8), Some(Pad::Up));
        assert_eq!(stick_direction(0.0, -0.7), Some(Pad::Down));
        assert_eq!(key_for(&Pad::Accept), Some(egui::Key::Enter));
        assert_eq!(key_for(&Pad::Start), None);
    }
}
