//! Colours and strength styling shared by annotations and the score sheet.
//! Criterion colours follow rubric order, matching the web UI.

use scorer_core::{Rubric, Strength, SubitemId};

const PALETTE: [(f32, f32, f32); 8] = [
    (0.145, 0.388, 0.922), // blue
    (0.086, 0.639, 0.290), // green
    (0.918, 0.345, 0.047), // orange
    (0.576, 0.200, 0.918), // purple
    (0.863, 0.149, 0.149), // red
    (0.031, 0.569, 0.698), // cyan
    (0.792, 0.541, 0.016), // amber
    (0.859, 0.153, 0.467), // pink
];

pub type Rgb = (f32, f32, f32);

pub fn criterion_color(index: usize) -> Rgb {
    PALETTE[index % PALETTE.len()]
}

pub fn subitem_color(rubric: &Rubric, subitem: &SubitemId) -> Rgb {
    rubric
        .criteria
        .iter()
        .position(|c| c.subitems.iter().any(|s| &s.id == subitem))
        .map(criterion_color)
        .unwrap_or((0.4, 0.4, 0.4))
}

/// Highlight fill opacity.
pub fn highlight_opacity(s: Strength) -> f32 {
    match s {
        Strength::Weak => 0.22,
        Strength::Moderate => 0.38,
        Strength::Strong => 0.58,
    }
}

/// Border width for figure boxes.
pub fn box_width(s: Strength) -> f32 {
    match s {
        Strength::Weak => 1.0,
        Strength::Moderate => 1.8,
        Strength::Strong => 2.8,
    }
}

pub fn letter(s: Strength) -> char {
    match s {
        Strength::Weak => 'W',
        Strength::Moderate => 'M',
        Strength::Strong => 'S',
    }
}
