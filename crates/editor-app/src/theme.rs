//! Original light and dark color themes.

use editor_syntax::Role;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSColor, NSView,
};
use objc2_foundation::NSArray;

type Rgb = (f64, f64, f64);

pub struct Theme {
    pub dark: bool,
    pub background: Rgb,
    pub text: Rgb,
    pub gutter_background: Rgb,
    pub gutter_text: Rgb,
    pub current_line: Rgb,
    pub selection: Rgb,
    pub caret: Rgb,
    pub bookmark: Rgb,
    pub locked_background: Rgb,
    pub issue: Rgb,
    pub find_match: Rgb,
    roles: fn(Role) -> Option<Rgb>,
}

fn light_roles(role: Role) -> Option<Rgb> {
    Some(match role {
        Role::Plain => return None,
        Role::Keyword => (0.55, 0.20, 0.75),
        Role::Command => (0.00, 0.35, 0.75),
        Role::OperationalCommand => (0.00, 0.48, 0.58),
        Role::SectionName => (0.62, 0.32, 0.00),
        Role::EntityName => (0.05, 0.47, 0.20),
        Role::Property => (0.10, 0.32, 0.58),
        Role::On => (0.10, 0.56, 0.22),
        Role::Off => (0.80, 0.16, 0.16),
        Role::Action => (0.86, 0.45, 0.00),
        Role::SeverityLow => (0.30, 0.55, 0.75),
        Role::SeverityMedium => (0.80, 0.55, 0.00),
        Role::SeverityHigh => (0.82, 0.10, 0.10),
        Role::Number => (0.08, 0.45, 0.66),
        Role::Network => (0.47, 0.25, 0.66),
        Role::String => (0.72, 0.22, 0.12),
        Role::StringEscape => (0.86, 0.42, 0.00),
        Role::CertBlock | Role::SecretValue => (0.60, 0.60, 0.60),
        Role::SecretTag => (0.76, 0.40, 0.00),
        Role::Comment => (0.45, 0.52, 0.45),
        Role::Framing => (0.58, 0.58, 0.64),
        Role::Type => (0.00, 0.46, 0.56),
        Role::Function => (0.30, 0.25, 0.76),
        Role::Variable => (0.18, 0.18, 0.20),
        Role::Operator => (0.38, 0.38, 0.40),
        Role::Tag => (0.00, 0.40, 0.72),
        Role::Attribute => (0.56, 0.36, 0.00),
        Role::DiffAdded => (0.10, 0.56, 0.22),
        Role::DiffRemoved => (0.80, 0.16, 0.16),
        Role::DiffHunk => (0.42, 0.30, 0.76),
    })
}

fn dark_roles(role: Role) -> Option<Rgb> {
    Some(match role {
        Role::Plain => return None,
        Role::Keyword => (0.80, 0.58, 1.00),
        Role::Command => (0.45, 0.70, 1.00),
        Role::OperationalCommand => (0.35, 0.82, 0.88),
        Role::SectionName => (1.00, 0.72, 0.42),
        Role::EntityName => (0.52, 0.86, 0.58),
        Role::Property => (0.62, 0.80, 1.00),
        Role::On => (0.45, 0.86, 0.48),
        Role::Off => (1.00, 0.45, 0.45),
        Role::Action => (1.00, 0.68, 0.30),
        Role::SeverityLow => (0.55, 0.78, 0.95),
        Role::SeverityMedium => (1.00, 0.80, 0.35),
        Role::SeverityHigh => (1.00, 0.40, 0.40),
        Role::Number => (0.50, 0.82, 1.00),
        Role::Network => (0.78, 0.62, 1.00),
        Role::String => (1.00, 0.62, 0.50),
        Role::StringEscape => (1.00, 0.76, 0.40),
        Role::CertBlock | Role::SecretValue => (0.50, 0.50, 0.52),
        Role::SecretTag => (1.00, 0.70, 0.35),
        Role::Comment => (0.50, 0.60, 0.50),
        Role::Framing => (0.52, 0.52, 0.58),
        Role::Type => (0.40, 0.85, 0.88),
        Role::Function => (0.70, 0.66, 1.00),
        Role::Variable => (0.86, 0.86, 0.88),
        Role::Operator => (0.70, 0.70, 0.72),
        Role::Tag => (0.45, 0.72, 1.00),
        Role::Attribute => (1.00, 0.78, 0.45),
        Role::DiffAdded => (0.45, 0.86, 0.48),
        Role::DiffRemoved => (1.00, 0.45, 0.45),
        Role::DiffHunk => (0.72, 0.62, 1.00),
    })
}

pub static LIGHT: Theme = Theme {
    dark: false,
    background: (1.0, 1.0, 1.0),
    text: (0.12, 0.12, 0.14),
    gutter_background: (0.965, 0.965, 0.97),
    gutter_text: (0.60, 0.60, 0.63),
    current_line: (0.955, 0.965, 0.985),
    selection: (0.72, 0.84, 1.0),
    caret: (0.0, 0.40, 1.0),
    bookmark: (0.20, 0.45, 0.95),
    locked_background: (0.93, 0.93, 0.95),
    issue: (0.90, 0.30, 0.20),
    find_match: (1.0, 0.92, 0.45),
    roles: light_roles,
};

pub static DARK: Theme = Theme {
    dark: true,
    background: (0.115, 0.12, 0.13),
    text: (0.87, 0.87, 0.89),
    gutter_background: (0.135, 0.14, 0.15),
    gutter_text: (0.45, 0.46, 0.50),
    current_line: (0.16, 0.17, 0.19),
    selection: (0.20, 0.34, 0.55),
    caret: (0.45, 0.72, 1.0),
    bookmark: (0.40, 0.62, 1.0),
    locked_background: (0.20, 0.20, 0.23),
    issue: (1.0, 0.45, 0.35),
    find_match: (0.55, 0.45, 0.10),
    roles: dark_roles,
};

impl Theme {
    pub fn role(&self, role: Role) -> Option<Rgb> {
        (self.roles)(role)
    }
}

pub fn color((r, g, b): Rgb) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0)
}

/// The theme matching the view's light or dark appearance.
pub fn for_view(view: &NSView) -> &'static Theme {
    let appearance: Retained<NSAppearance> = view.effectiveAppearance();
    let names = unsafe { NSArray::from_slice(&[NSAppearanceNameAqua, NSAppearanceNameDarkAqua]) };
    let best = appearance.bestMatchFromAppearancesWithNames(&names);
    if best.is_some_and(|name| &*name == unsafe { NSAppearanceNameDarkAqua }) {
        &DARK
    } else {
        &LIGHT
    }
}
