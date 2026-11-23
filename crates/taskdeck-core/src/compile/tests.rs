//! `compile::trim` and `compile::detab` (core.md §2, §13.3; brief c2 item 4).
//! The rest of `compile` is task D.

use super::*;

/// The 25 White_Space code points of core.md §2, written out from the spec's
/// text: U+0009 to U+000D, U+0020, U+0085, U+00A0, U+1680, U+2000 to U+200A,
/// U+2028, U+2029, U+202F, U+205F and U+3000.
fn spec_white_space() -> Vec<char> {
    let mut out: Vec<char> = (0x09..=0x0D).filter_map(char::from_u32).collect();
    out.extend(['\u{20}', '\u{85}', '\u{A0}', '\u{1680}']);
    out.extend((0x2000..=0x200A).filter_map(char::from_u32));
    out.extend(['\u{2028}', '\u{2029}', '\u{202F}', '\u{205F}', '\u{3000}']);
    out
}

// spec: CORE-COMP-002
#[test]
fn trim_removes_each_of_the_25_white_space_code_points_from_both_ends() {
    let all = spec_white_space();
    assert_eq!(all.len(), 25);
    for c in &all {
        let text = format!("{c}{c}a{c}b{c}");
        assert_eq!(trim(&text), format!("a{c}b"), "U+{:04X}", u32::from(*c));
        assert_eq!(trim(&c.to_string()), "", "U+{:04X} alone", u32::from(*c));
    }
    // All of them together, in a run, at both ends.
    let run: String = all.iter().collect();
    assert_eq!(trim(&format!("{run}name{run}")), "name");
}

// spec: CORE-COMP-002
#[test]
fn trim_keeps_zero_width_space_and_the_byte_order_mark() {
    // Neither is White_Space (core.md §2), so neither is trimmed.
    for c in ['\u{200B}', '\u{FEFF}'] {
        let text = format!("{c} a {c}");
        assert_eq!(trim(&text), text, "U+{:04X}", u32::from(c));
        assert_eq!(trim(&format!(" {c}a{c} ")), format!("{c}a{c}"));
    }
    // Nor anything else outside the list: U+180E (no longer White_Space),
    // U+200C, U+2060.
    for c in ['\u{180E}', '\u{200C}', '\u{2060}'] {
        let text = format!("{c}a{c}");
        assert_eq!(trim(&text), text, "U+{:04X}", u32::from(c));
    }
}

// spec: CORE-COMP-002
#[test]
fn trim_only_touches_the_ends() {
    assert_eq!(trim(""), "");
    assert_eq!(trim("a"), "a");
    assert_eq!(trim("\u{3000}a \t b\u{2028}"), "a \t b");
}

#[test]
fn detab_replaces_every_tab_by_four_spaces() {
    assert_eq!(detab("a\tb"), "a    b");
    assert_eq!(detab("\t\t"), "        ");
    assert_eq!(detab("none"), "none");
    // Only U+0009: the other White_Space code points stay.
    assert_eq!(detab("\u{B}\u{A0}"), "\u{B}\u{A0}");
}
