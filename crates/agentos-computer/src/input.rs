//! What an agent may ask the keyboard and mouse to do.
//!
//! The vocabulary is closed and platform-independent on purpose. Two reasons:
//!
//! * The model is given an enumerated set of legal values, so "press the key
//!   called `\u{1b}[3~`" is a validation failure rather than something the
//!   backend has to interpret.
//! * The keys named here exist on both macOS and Windows. A vocabulary borrowed
//!   from one platform's key table would have let a policy be written against
//!   keys the other cannot press.
//!
//! Everything is measured in **points**, the same units the operating system
//! uses for the cursor. On a display with a scale factor those are not pixels:
//! a screenshot of a 1728×1117-point display is 3456×2234 pixels, and clicking
//! at a coordinate read off that image would land at twice the intended offset.
//! [`crate::Capture`] reports both, so a caller can convert.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A position on the desktop, in points, with the origin at the top left of the
/// primary display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    /// Horizontal offset.
    pub x: i32,
    /// Vertical offset.
    pub y: i32,
}

impl Point {
    /// Build a point.
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

/// A mouse button.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Button {
    /// The primary button.
    #[default]
    Left,
    /// The secondary button, which opens context menus.
    Right,
    /// The wheel button.
    Middle,
}

impl fmt::Display for Button {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Middle => "middle",
        })
    }
}

/// A modifier held while another key is pressed.
///
/// Named after the physical key, not after its role: the shortcut modifier is
/// Command on macOS and Control on Windows, and a policy or an approval card
/// that said "the shortcut modifier" would be telling the operator less than it
/// knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    /// Shift.
    Shift,
    /// Control.
    Control,
    /// Alt, which is Option on macOS.
    Alt,
    /// Command on macOS, the Windows key on Windows.
    Command,
}

impl fmt::Display for Modifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Shift => "shift",
            Self::Control => "control",
            Self::Alt => "alt",
            Self::Command => "command",
        })
    }
}

/// A key that can be pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A printable character, entered whatever the current layout is.
    Character(char),
    /// Escape.
    Escape,
    /// Tab.
    Tab,
    /// Return or Enter.
    Return,
    /// Space.
    Space,
    /// Backspace, deleting backwards.
    Backspace,
    /// Forward delete.
    Delete,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// A function key, `F1` to `F12`.
    Function(u8),
}

/// Every key name the parser accepts, besides a single character and `f1`–`f12`.
const NAMED_KEYS: &[(&str, Key)] = &[
    ("escape", Key::Escape),
    ("tab", Key::Tab),
    ("return", Key::Return),
    ("enter", Key::Return),
    ("space", Key::Space),
    ("backspace", Key::Backspace),
    ("delete", Key::Delete),
    ("up", Key::Up),
    ("down", Key::Down),
    ("left", Key::Left),
    ("right", Key::Right),
    ("home", Key::Home),
    ("end", Key::End),
    ("page_up", Key::PageUp),
    ("page_down", Key::PageDown),
];

impl Key {
    /// Parse a key name.
    ///
    /// Accepts one of the names in [`Self::names`], `f1` through `f12`, or a
    /// single character. Anything else is refused rather than guessed at.
    ///
    /// # Errors
    ///
    /// The offending text, for an argument-validation message.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let trimmed = raw.trim();
        let lowered = trimmed.to_ascii_lowercase();

        if let Some((_, key)) = NAMED_KEYS.iter().find(|(name, _)| *name == lowered) {
            return Ok(*key);
        }

        if let Some(number) = lowered.strip_prefix('f')
            && let Ok(index) = number.parse::<u8>()
            && (1..=12).contains(&index)
        {
            return Ok(Self::Function(index));
        }

        // A single character is the portable way to press a letter, a digit or
        // punctuation: the platform key tables disagree about everything else.
        let mut characters = trimmed.chars();
        match (characters.next(), characters.next()) {
            (Some(character), None) if !character.is_control() => Ok(Self::Character(character)),
            _ => Err(format!(
                "`{raw}` is not a key; use one of {}, f1-f12, or a single character",
                Self::names().join(", ")
            )),
        }
    }

    /// The accepted named keys, for schemas and error messages.
    #[must_use]
    pub fn names() -> Vec<&'static str> {
        NAMED_KEYS.iter().map(|(name, _)| *name).collect()
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Character(character) => write!(f, "{character}"),
            Self::Function(index) => write!(f, "f{index}"),
            other => f.write_str(
                NAMED_KEYS
                    .iter()
                    .find(|(_, key)| key == other)
                    .map_or("?", |(name, _)| *name),
            ),
        }
    }
}

/// Which way a scroll goes.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// Up and down.
    #[default]
    Vertical,
    /// Left and right.
    Horizontal,
}

/// One thing to do with the mouse or the keyboard.
///
/// The backend performs these and nothing else, so every way an agent can reach
/// the physical machine is a variant of this enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputAction {
    /// Move the cursor.
    Move {
        /// Where to.
        to: Point,
    },
    /// Press and release a button, optionally moving first.
    Click {
        /// Which button.
        button: Button,
        /// Where, or the current position if absent.
        at: Option<Point>,
        /// How many presses: 1 for a click, 2 for a double click.
        count: u8,
    },
    /// Press at one point, move, release at another.
    Drag {
        /// Which button.
        button: Button,
        /// Where the press happens.
        from: Point,
        /// Where the release happens.
        to: Point,
    },
    /// Turn the wheel.
    Scroll {
        /// Which axis.
        axis: Axis,
        /// Wheel clicks; positive is down or right.
        amount: i32,
    },
    /// Enter text.
    Type {
        /// The text.
        text: String,
    },
    /// Press one key, with modifiers held around it.
    Key {
        /// The key.
        key: Key,
        /// Modifiers held while it is pressed.
        modifiers: Vec<Modifier>,
    },
}

impl fmt::Display for InputAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Move { to } => write!(f, "move the cursor to {to}"),
            Self::Click { button, at, count } => {
                let what = if *count > 1 {
                    format!("{count}x {button} click")
                } else {
                    format!("{button} click")
                };
                match at {
                    Some(point) => write!(f, "{what} at {point}"),
                    None => write!(f, "{what} where the cursor is"),
                }
            }
            Self::Drag { button, from, to } => write!(f, "drag {button} from {from} to {to}"),
            Self::Scroll { axis, amount } => {
                let direction = match (axis, amount.is_negative()) {
                    (Axis::Vertical, false) => "down",
                    (Axis::Vertical, true) => "up",
                    (Axis::Horizontal, false) => "right",
                    (Axis::Horizontal, true) => "left",
                };
                write!(f, "scroll {direction} by {}", amount.abs())
            }
            Self::Type { text } => write!(f, "type {} character(s)", text.chars().count()),
            Self::Key { key, modifiers } => {
                for modifier in modifiers {
                    write!(f, "{modifier}-")?;
                }
                write!(f, "{key}")
            }
        }
    }
}

impl InputAction {
    /// How many separate events this action delivers.
    ///
    /// The focus check is repeated before each one, so this is also the number
    /// of chances the target has to change underneath it. It is the honest
    /// measure of how long an action is exposed, and it is why typing a long
    /// string is more dangerous than pressing one key.
    #[must_use]
    pub fn event_count(&self) -> usize {
        match self {
            Self::Move { .. } | Self::Scroll { .. } => 1,
            Self::Click { count, .. } => usize::from(*count),
            Self::Drag { .. } => 3,
            Self::Type { text } => text.chars().count(),
            Self::Key { modifiers, .. } => modifiers.len().saturating_mul(2).saturating_add(1),
        }
    }

    /// Whether this keystroke commits what has been typed.
    ///
    /// Return sends the message. `browser.type` draws the same distinction with
    /// `submit: true` and raises its risk accordingly; this is the keyboard's
    /// version of it, and it is the only part of "what will this do?" that a
    /// keystroke reveals.
    ///
    /// It says nothing about the mouse. What a click at a coordinate commits is
    /// not knowable from the arguments, which is a limitation of the whole idea
    /// rather than of this function — see `SECURITY.md`.
    #[must_use]
    pub fn commits(&self) -> bool {
        match self {
            Self::Type { text } => text.contains('\n') || text.contains('\r'),
            Self::Key { key, .. } => matches!(key, Key::Return),
            Self::Move { .. } | Self::Scroll { .. } | Self::Click { .. } | Self::Drag { .. } => {
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keys_parse_case_insensitively() {
        assert_eq!(Key::parse("Escape"), Ok(Key::Escape));
        assert_eq!(Key::parse("page_down"), Ok(Key::PageDown));
        assert_eq!(Key::parse("enter"), Ok(Key::Return));
        assert_eq!(Key::parse(" tab "), Ok(Key::Tab));
    }

    #[test]
    fn function_keys_parse_within_range() {
        assert_eq!(Key::parse("f1"), Ok(Key::Function(1)));
        assert_eq!(Key::parse("F12"), Ok(Key::Function(12)));
        assert!(Key::parse("f0").is_err());
        assert!(Key::parse("f13").is_err());
        // `f` alone is the letter, not a malformed function key.
        assert_eq!(Key::parse("f"), Ok(Key::Character('f')));
    }

    #[test]
    fn single_characters_parse_and_longer_strings_do_not() {
        assert_eq!(Key::parse("a"), Ok(Key::Character('a')));
        assert_eq!(Key::parse("/"), Ok(Key::Character('/')));
        assert_eq!(Key::parse("é"), Ok(Key::Character('é')));
        // A chord is expressed with `modifiers`, not smuggled into the key.
        assert!(Key::parse("cmd+c").is_err());
        assert!(Key::parse("").is_err());
        assert!(Key::parse("\u{7}").is_err());
    }

    #[test]
    fn keys_render_the_way_they_are_written() {
        assert_eq!(Key::Escape.to_string(), "escape");
        assert_eq!(Key::Function(5).to_string(), "f5");
        assert_eq!(Key::Character('c').to_string(), "c");
        // `enter` is an accepted alias, but the canonical spelling is `return`.
        assert_eq!(Key::Return.to_string(), "return");
    }

    #[test]
    fn actions_describe_themselves_for_an_approval_card() {
        assert_eq!(
            InputAction::Key {
                key: Key::Character('c'),
                modifiers: vec![Modifier::Command],
            }
            .to_string(),
            "command-c"
        );
        assert_eq!(
            InputAction::Click {
                button: Button::Left,
                at: Some(Point::new(12, 34)),
                count: 2,
            }
            .to_string(),
            "2x left click at (12, 34)"
        );
        assert_eq!(
            InputAction::Scroll {
                axis: Axis::Vertical,
                amount: -3,
            }
            .to_string(),
            "scroll up by 3"
        );
    }

    #[test]
    fn typing_counts_one_event_per_character() {
        // The number of chances focus has to change while the action runs.
        let action = InputAction::Type {
            text: "hello".to_owned(),
        };
        assert_eq!(action.event_count(), 5);
        assert_eq!(
            InputAction::Key {
                key: Key::Character('c'),
                modifiers: vec![Modifier::Command],
            }
            .event_count(),
            3
        );
    }

    #[test]
    fn keystrokes_that_commit_are_recognised() {
        assert!(
            InputAction::Type {
                text: "send it\n".to_owned()
            }
            .commits()
        );
        assert!(
            !InputAction::Type {
                text: "a draft".to_owned()
            }
            .commits()
        );
        assert!(
            InputAction::Key {
                key: Key::Return,
                modifiers: vec![],
            }
            .commits()
        );
        assert!(
            !InputAction::Key {
                key: Key::Character('a'),
                modifiers: vec![Modifier::Command],
            }
            .commits()
        );
        assert!(
            !InputAction::Scroll {
                axis: Axis::Vertical,
                amount: 3
            }
            .commits()
        );
    }
}
