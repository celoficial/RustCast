use crossterm::event::KeyEvent;

use crate::discovery::health::PollSignal;

pub enum TuiEvent {
    Key(KeyEvent),
    Tick,
    Poll(PollSignal),
}
