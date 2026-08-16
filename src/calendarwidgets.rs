use egui::{self, Align2, Color32, CornerRadius, Rect, Shape, Stroke, StrokeKind, Widget};
use emath;
use epaint::TextShape;
use egui::{FontId, Ui, Pos2, FontFamily, vec2, Sense};

/* ─────────────────────── Fitting a name onto a card ───────────────────────
 *
 * Every card here has the same problem: a name of any length, a box about
 * thirteen characters wide, and a shape that is not a rectangle — the day
 * number is punched out of one corner, so the first row is narrower than the
 * ones under it.
 *
 * Each of the four card widgets used to solve it separately, and all four were
 * wrong in the same way and some in more:
 *
 *   - Only the **first** row was measured. Everything left over was poured into
 *     the second with no width check at all, so a long name ran off the side of
 *     the card and was clipped mid-glyph.
 *   - Anything past two rows was dropped in silence — no ellipsis, no sign that
 *     the name had ever continued.
 *   - A word wider than a row (`Supercalifragilisticexpialidocious`) never fit
 *     the first row, so the first row was left *empty* and the whole word went
 *     to the second, where it overflowed.
 *   - Two of the four had no "stop filling the first row" flag, so once one
 *     word failed to fit, later *shorter* words were still appended to row one
 *     — printing the name with its words **out of order**.
 *   - One of those two appended to row two with `push_str(word)` and no
 *     separator, so it printed them **run together**: "overdueprojectretro".
 *
 * One implementation now, taking the width of each row it is allowed to use.
 */

/// What stands in for the part of a name that did not fit.
const ELLIPSIS: char = '…';

/// The face every card's text is set in.
const CARD_TEXT_SIZE: f32 = 11.0;

/// Row pitch for it. Space Mono at 11 points is about 13 tall; this is the step
/// from one row of a card to the next.
const CARD_LINE_HEIGHT: f32 = 13.0;

fn card_text_font() -> FontId {
    FontId::new(CARD_TEXT_SIZE, FontFamily::Name("space".into()))
}

/// Lay `text` out into rows of the given widths: breaking at spaces where it
/// can, inside a word where it must, and ending in `…` when something was left
/// over.
///
/// `widths` has one entry per row the card has room for, **in order** — which
/// is what lets a card whose first row is shortened by the day number use its
/// full width further down.
pub fn fit_text_rows(ui: &Ui, text: &str, font: &FontId, widths: &[f32]) -> Vec<String> {
    // The font lock is taken once for the whole layout rather than per glyph.
    ui.fonts_mut(|fonts| {
        let mut advance = |ch: char| fonts.glyph_width(font, ch);
        fit_rows_by(text, &mut advance, widths)
    })
}

/// The layout itself, over a per-character advance rather than a font, so the
/// rules can be tested against a known-width character instead of against
/// whatever the font happens to measure.
///
/// Summing advances is how egui's own layouter measures a row, so this agrees
/// with what is actually painted; it ignores kerning, which egui's basic path
/// ignores too.
fn fit_rows_by(
    text: &str,
    advance: &mut impl FnMut(char) -> f32,
    widths: &[f32],
) -> Vec<String> {
    let mut pending: std::collections::VecDeque<&str> = text.split_whitespace().collect();
    let mut rows = Vec::with_capacity(widths.len());

    for (index, &width) in widths.iter().enumerate() {
        if pending.is_empty() {
            break;
        }
        let last = index + 1 == widths.len();
        rows.push(take_row(&mut pending, advance, width, last));
    }
    rows
}

/// Fill one row from the front of `pending`.
fn take_row(
    pending: &mut std::collections::VecDeque<&str>,
    advance: &mut impl FnMut(char) -> f32,
    width: f32,
    last: bool,
) -> String {
    let space = advance(' ');
    let mut row = String::new();
    let mut used = 0.0;

    // Whole words, while they fit. Each is measured once, so a card costs a
    // pass over its own name however many rows it takes.
    while let Some(&word) = pending.front() {
        let word_width = measure(word, advance);
        let gap = if row.is_empty() { 0.0 } else { space };
        if used + gap + word_width > width {
            break;
        }
        if !row.is_empty() {
            row.push(' ');
        }
        row.push_str(word);
        used += gap + word_width;
        pending.pop_front();
    }

    if row.is_empty() {
        // Nothing fits, so the next word is wider than a whole row and has to
        // be broken inside. A name with no space in it has to be cut somewhere,
        // and cutting it here — rather than leaving the row empty and hoping
        // the next one is wider — is what keeps it on the card.
        let word = pending.pop_front().unwrap_or_default();
        let budget = if last { width - measure_char(ELLIPSIS, advance) } else { width };
        // At least one character, always: a row that takes nothing makes no
        // progress, and the loop above would ask for the same word forever.
        let head_len = prefix_that_fits(word, advance, budget).max(first_char_len(word));
        let (head, tail) = word.split_at(head_len.min(word.len()));
        row.push_str(head);
        used = measure(head, advance);
        if !tail.is_empty() {
            pending.push_front(tail);
        }
    }

    if last && !pending.is_empty() {
        pending.clear();
        return with_ellipsis(row, used, advance, width);
    }
    row
}

fn measure(text: &str, advance: &mut impl FnMut(char) -> f32) -> f32 {
    text.chars().map(|ch| advance(ch)).sum()
}

fn measure_char(ch: char, advance: &mut impl FnMut(char) -> f32) -> f32 {
    advance(ch)
}

/// Byte length of the longest prefix of `text` that fits `budget`.
fn prefix_that_fits(text: &str, advance: &mut impl FnMut(char) -> f32, budget: f32) -> usize {
    let mut used = 0.0;
    let mut fitted = 0;
    for (index, ch) in text.char_indices() {
        let width = advance(ch);
        if used + width > budget {
            break;
        }
        used += width;
        fitted = index + ch.len_utf8();
    }
    fitted
}

fn first_char_len(text: &str) -> usize {
    text.chars().next().map_or(0, char::len_utf8)
}

/// Trim the row back until the ellipsis fits beside it, then add it.
fn with_ellipsis(
    mut row: String,
    mut used: f32,
    advance: &mut impl FnMut(char) -> f32,
    width: f32,
) -> String {
    let mark = measure_char(ELLIPSIS, advance);
    while used + mark > width {
        match row.pop() {
            Some(ch) => used -= advance(ch),
            None => break,
        }
    }
    // "half a name …" reads as a typo; the ellipsis belongs against the text.
    while row.ends_with(' ') {
        row.pop();
    }
    row.push(ELLIPSIS);
    row
}

/// How many rows of text fit between `top` and `bottom`.
///
/// Measured rather than assumed. The number of rows a card has room for depends
/// on the height of the day number's galley, which depends on the font and on
/// the UI scale — guess it and the last row straddles the bottom edge of the
/// card and is painted in half, which is exactly what a first attempt at this
/// did.
fn rows_between(top: f32, bottom: f32) -> usize {
    (((bottom - top) / CARD_LINE_HEIGHT).floor()).max(0.0) as usize
}

/// Paint rows produced by `fit_text_rows` down a card, one `CARD_LINE_HEIGHT`
/// apart, starting at `first`. `lefts` gives each row its own left edge, so a
/// row indented past the day number and the full-width rows below it line up
/// with the widths they were fitted to.
fn paint_rows(
    painter: &egui::Painter,
    rows: &[String],
    lefts: &[f32],
    first: Pos2,
    font: &FontId,
    color: Color32,
) {
    for (index, row) in rows.iter().enumerate() {
        let left = lefts.get(index).copied().unwrap_or(first.x);
        let pos = Pos2::new(left, first.y + index as f32 * CARD_LINE_HEIGHT);
        painter.text(pos, Align2::LEFT_TOP, row, font.clone(), color);
    }
}

pub struct DayNumber<'a> {
    pub number: &'a str,
    pub is_strong: bool,
}

impl<'a> DayNumber<'a> {
    pub fn new(number: &'a str, is_strong: bool) -> Self {
        Self { number, is_strong }
    }
}

impl<'a> egui::Widget for DayNumber<'a> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let desired_size = vec2(ui.available_width(), 60.0); // same height as DayHeader
        let (rect, response) = ui.allocate_exact_size(desired_size, Sense::hover());
        let painter = ui.painter_at(rect);

        let number_pos = Pos2::new(rect.left() + 5.0, rect.top() + 5.0);

        // Choose font and color
        let color = if self.is_strong {
            ui.style().visuals.strong_text_color()
        } else {
            ui.style().visuals.text_color()
        };
        let font_id = FontId {
            size: 16.0,
            family: FontFamily::Name("anton".into()),
        };

        // Layout number
        let number_galley = ui.fonts_mut( |f| {
            f.layout_no_wrap(self.number.to_string(), font_id.clone(), color)
        });

        // Draw number
        painter.galley(number_pos, number_galley, Color32::WHITE);

        response
    }
}


pub struct DayHeader<'a> {
    pub number: &'a str,
    pub text: &'a str,
    pub is_strong: bool,
    pub hour: &'a str,
    pub color: Color32,
}

impl<'a> DayHeader<'a> {
    pub fn new(number: &'a str, text: &'a str, is_strong: bool, hour: &'a str, color: Color32) -> Self {
        Self { number, text, is_strong, hour, color }
    }
}

impl<'a> egui::Widget for DayHeader<'a> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let desired_size = vec2(ui.available_width(), 60.0);
        let (rect, response) = ui.allocate_exact_size(desired_size, Sense::hover());

        let painter = ui.painter_at(rect);

        let text_font = card_text_font();

        let margin = 5.0;
        let number_pos = Pos2::new(rect.left() + margin, rect.top() + margin);

        // Measure number
        let number_galley = ui.fonts_mut(|f| {
            let color = if self.is_strong {
                ui.style().visuals.strong_text_color()
            } else {
                ui.style().visuals.text_color()
            };

            let font_id = FontId { size: 16.0, family: FontFamily::Name("anton".into()) };
            f.layout_no_wrap(self.number.to_string(), font_id, color)
        });

        let number_size = number_galley.size();

        // Draw the text outline as a custom shape (non-rectangular)
        let path = {
            let mut path = Vec::new();

            //rounded
            let radius = 8.0; // change this for more or less rounding
            let corner_start = Pos2::new(rect.right() - radius, rect.top());
            let corner_center = Pos2::new(rect.right() - radius, rect.top() + radius);

            // Start before the curve
            path.push(Pos2::new(number_pos.x + number_size.x + margin, rect.top()));
            path.push(corner_start);

            // Add top-right arc as a series of points (quarter circle)
            let segments = 5; // more segments = smoother corner
            for i in 0..=segments {
                let t = i as f32 / segments as f32;
                let angle = std::f32::consts::FRAC_PI_2 * t; // 90 degrees (π/2)
                let x = corner_center.x + radius * angle.sin();
                let y = corner_center.y - radius * angle.cos();
                path.push(Pos2::new(x, y));
            }
            //
            
            path.push(Pos2::new(rect.right(), rect.bottom()));
            path.push(Pos2::new(rect.left(), rect.bottom()));
            path.push(Pos2::new(rect.left(), number_pos.y + number_size.y + margin));
            path.push(Pos2::new(number_pos.x + number_size.x + margin, number_pos.y + number_size.y + margin));
            path.push(Pos2::new(number_pos.x + number_size.x + margin, rect.top()));
            path
        };

        // Background fill
        let bg_color = self.color;

        painter.add(Shape::convex_polygon(path.clone(), bg_color, Stroke::NONE));

        // Outline with slightly rounded appearance (stroke overlays the filled shape)
        let stroke_color = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let stroke = Stroke::new(1.0, stroke_color);
        painter.add(Shape::closed_line(path, stroke));



        // Paint the number
        painter.galley(number_pos, number_galley, Color32::WHITE);

        // The name, in a shape that is not a rectangle: the first row starts
        // after the day number, the rows under it get the whole card. Both are
        // measured, which is the difference between this and what was here
        // before — the second row was never measured at all, so a long name ran
        // off the side and was clipped mid-glyph.
        let color = Color32::from_gray(150);

        let indented_left = number_pos.x + number_size.x + margin * 2.0 + 2.0;
        let full_left = rect.left() + margin + 2.0;
        let right = rect.right() - margin;

        let first_row = Pos2::new(indented_left, number_pos.y + 5.0);
        // Below the number, which is what frees the full width — and however
        // many of those the card actually has room for. The galley of a
        // 16-point Anton numeral is a few points taller than its glyphs, so
        // starting flush under it wastes the row that space would have paid
        // for; `- 4` is that leading given back, and the count is measured
        // rather than assumed so it stays right at any UI scale.
        let lower_rows_top = number_pos.y + number_size.y - 4.0;
        let lower_rows = rows_between(lower_rows_top, rect.bottom() - 2.0);

        let mut widths = vec![right - indented_left];
        widths.extend(std::iter::repeat_n(right - full_left, lower_rows));
        let rows = fit_text_rows(ui, self.text, &text_font, &widths);

        if let Some(first) = rows.first() {
            painter.text(first_row, Align2::LEFT_TOP, first, text_font.clone(), color);
        }
        if rows.len() > 1 {
            paint_rows(
                &painter,
                &rows[1..],
                &vec![full_left; lower_rows],
                Pos2::new(full_left, lower_rows_top),
                &text_font,
                color,
            );
        }

        // Dynamic hour string (optional, can be from a field)
        // 1. Compute your external position
        let hourmark_pos = Pos2::new(
            rect.center().x + 22.0,
            rect.top() - 3.0,
        );

        // 2. Prepare background and text layout
        let hour_label = self.hour.to_string();
        let hour_font = FontId {
            size: 10.0,
            family: FontFamily::Name("space".into()),
        };
        let hour_size = ui.fonts_mut(|f| f.layout_no_wrap(hour_label.clone(), hour_font.clone(), color).size());
        let hour_padding = 3.0;

        let bg_rect = Rect::from_min_size(
            hourmark_pos - vec2(hour_padding, hour_padding / 2.0),
            hour_size + vec2(hour_padding * 2.0, hour_padding),
        );

        // 3. Create a painter with an **infinite clip rect**
        let unclipped_painter = ui.painter().with_clip_rect(Rect::EVERYTHING);

        // 4. Draw outside the original bounds safely
        unclipped_painter.rect_filled(bg_rect, 6.0, Color32::from_black_alpha(40));
        unclipped_painter.rect_stroke(bg_rect, 6.0, Stroke::new(0.1, Color32::from_white_alpha(120)), StrokeKind::Middle);
        unclipped_painter.text(hourmark_pos, Align2::LEFT_TOP, hour_label, hour_font, Color32::from_white_alpha(150));

        response
    }
}


pub struct MiddleHeader<'a> {
    pub text: &'a str,
    pub hour: Option<&'a str>,
    pub color: Color32,
}

impl<'a> MiddleHeader<'a> {
    pub fn new(text: &'a str, hour: Option<&'a str>, color: Color32) -> Self {
        Self { text, hour, color }
    }
}

impl<'a> egui::Widget for MiddleHeader<'a> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {

        let desired_size = vec2(ui.available_width(), 60.0);
        let (rect, response) = ui.allocate_exact_size(desired_size, Sense::hover());
        let painter = ui.painter_at(rect);

        // let bg_color = ui.visuals().widgets.hovered.bg_fill;
        let bg_color = self.color;

        let stroke = ui.visuals().widgets.noninteractive.bg_stroke;

        let rounding = CornerRadius::same(6);
        painter.rect(rect, rounding, bg_color, stroke, StrokeKind::Inside);

        let text_font = card_text_font();
        let color = Color32::from_gray(150);

        // A plain rectangle, so every row is the same width — and three of them
        // fit inside the card's 60 points. The old pair sat 18 points apart,
        // which is a line and a half for an 11-point face: it looked airy and
        // it cost a third of the card's text for nothing.
        let margin = 12.0;
        let left = rect.left() + margin;
        let text_width = rect.width() - margin * 2.0;

        let top = rect.top() + 10.0;
        let count = rows_between(top, rect.bottom() - 8.0);
        let rows = fit_text_rows(ui, self.text, &text_font, &vec![text_width; count]);
        paint_rows(&painter, &rows, &vec![left; count], Pos2::new(left, top), &text_font, color);

        if let Some(hour) = self.hour {
            // 1. Compute your external position
            let hourmark_pos = Pos2::new(
                rect.center().x - 59.0,
                rect.bottom() - 9.0,
            );

            // 2. Prepare background and text layout
            let hour_label = hour.to_string();
            let hour_font = FontId {
                size: 10.0,
                family: FontFamily::Name("space".into()),
            };
            let hour_size = ui.fonts_mut(|f| f.layout_no_wrap(hour_label.clone(), hour_font.clone(), color).size());
            let hour_padding = 3.0;

            let bg_rect = Rect::from_min_size(
                hourmark_pos - vec2(hour_padding, hour_padding / 2.0),
                hour_size + vec2(hour_padding * 2.0, hour_padding),
            );

            // 3. Create a painter with an **infinite clip rect**
            let unclipped_painter = ui.painter().with_clip_rect(Rect::EVERYTHING);

            // 4. Draw outside the original bounds safely
            unclipped_painter.rect_filled(bg_rect, 6.0, Color32::from_black_alpha(40));
            unclipped_painter.rect_stroke(bg_rect, 6.0, Stroke::new(0.1, Color32::from_white_alpha(120)), StrokeKind::Middle);
            unclipped_painter.text(hourmark_pos, Align2::LEFT_TOP, hour_label, hour_font, Color32::from_white_alpha(150));
        }        

        response
    }
}


pub struct RotatedNumberOnly<'a> {
    pub number: &'a str,
    pub is_strong: bool,
}

impl<'a> RotatedNumberOnly<'a> {
    pub fn new(number: &'a str, is_strong: bool) -> Self {
        Self { number, is_strong }
    }
}

impl<'a> Widget for RotatedNumberOnly<'a> {
    fn ui(self, ui: &mut Ui) -> egui::Response {
        let desired_size = vec2(ui.available_width(), 60.0);
        let (rect, response) = ui.allocate_exact_size(desired_size, Sense::hover());

        let painter = ui.painter_at(rect);
        let margin = 7.0;

        // Prepare the number galley (rotated)
        let number_galley = ui.fonts_mut(|f| {
            let color = if self.is_strong {
                ui.style().visuals.strong_text_color()
            } else {
                ui.style().visuals.text_color()
            };
            let font_id = FontId {
                size: 16.0,
                family: FontFamily::Name("anton".into()),
            };
            f.layout_no_wrap(self.number.to_string(), font_id, color)
        });

        let number_size = number_galley.size();
        let rotation = emath::Rot2::from_angle(std::f32::consts::PI); // 180°

        // Position the rotated number in the same place as before
        let rotated_bb = Rect::from_center_size(Pos2::ZERO, number_size).rotate_bb(rotation);
        let rotated_size = rotated_bb.size();

        let number_center = Pos2::new(
            rect.right() - margin - rotated_size.x / 2.0,
            rect.bottom() - margin - rotated_size.y / 2.0,
        );

        let number_pos = number_center - (rotation * (number_size / 2.0));

        painter.add(TextShape {
            galley: number_galley,
            pos: number_pos,
            angle: std::f32::consts::PI,
            underline: Stroke::default(),
            fallback_color: Color32::WHITE,
            opacity_factor: 1.0,
            override_text_color: None,
        });

        response
    }
}


pub struct BottomHeaderRotated<'a> {
    pub number: &'a str,
    pub text: &'a str,
    pub is_strong: bool,
    pub hour: &'a str,
    pub top_hour: Option<&'a str>,
    pub color: Color32,
}

impl<'a> BottomHeaderRotated<'a> {
    pub fn new(number: &'a str, text: &'a str, is_strong: bool, hour: &'a str, top_hour: Option<&'a str>, color: Color32) -> Self {
        Self { number, text, is_strong, hour, top_hour, color }
    }
}

impl<'a> egui::Widget for BottomHeaderRotated<'a> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let desired_size = vec2(ui.available_width(), 60.0);
        let (rect, response) = ui.allocate_exact_size(desired_size, Sense::hover());

        let painter = ui.painter_at(rect);

        let text_font = card_text_font();

        // Margin and positioning
        let margin = 7.0;

        // Measure number galley (rotated)
        let number_galley = ui.fonts_mut(|f| {
            let color = if self.is_strong {
                ui.style().visuals.strong_text_color()
            } else {
                ui.style().visuals.text_color()
            };

            // let font_id = FontSelection::Default.resolve(ui.style());
            let font_id = FontId { size: 16.0, family: FontFamily::Name("anton".into()) };
            f.layout_no_wrap(self.number.to_string(), font_id, color)
        });


        let number_size = number_galley.size();
        let rotation = egui::emath::Rot2::from_angle(std::f32::consts::PI); // 180 degrees

        // Position rotated number at bottom right
        let rotated_bb = Rect::from_center_size(Pos2::ZERO, number_size).rotate_bb(rotation);
        let rotated_size = rotated_bb.size();

        let number_center = Pos2::new(
            rect.right() - margin - rotated_size.x / 2.0,
            rect.bottom() - margin - rotated_size.y / 2.0 + 4.0,
        );

        // Calculate surrounding area for the path
        let path = {
            let mut path = Vec::new();
            let radius = 8.0;
            let segments = 5; // More segments = smoother curve

            // Arc center is inset from bottom-left corner
            let arc_center = Pos2::new(rect.left() + radius, rect.bottom() - radius);

            // Start of arc (horizontal line end)
            path.push(Pos2::new(number_center.x - rotated_size.x / 2.0 - margin, rect.bottom()));
            path.push(Pos2::new(arc_center.x, rect.bottom()));

            // Bottom-left arc: 90° curve from bottom to left
            for i in 0..=segments {
                let t = i as f32 / segments as f32;
                let angle = std::f32::consts::FRAC_PI_2 * (1.0 - t); // From 90° to 0°
                let x = arc_center.x - radius * angle.cos();
                let y = arc_center.y + radius * angle.sin();
                path.push(Pos2::new(x, y));
            }
            //

            path.push(Pos2::new(rect.left(), rect.top()));
            path.push(Pos2::new(rect.right(), rect.top()));
            path.push(Pos2::new(rect.right(), number_center.y - rotated_size.y / 2.0 - margin));
            path.push(Pos2::new(number_center.x - rotated_size.x / 2.0 - margin, number_center.y - rotated_size.y / 2.0 - margin));
            path.push(Pos2::new(number_center.x - rotated_size.x / 2.0 - margin, rect.bottom()));
            path
        };

        // Background fill
        let bg_color = self.color;

        painter.add(Shape::convex_polygon(path.clone(), bg_color, Stroke::NONE));

        // Outline stroke
        let stroke_color = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let stroke = Stroke::new(1.0, stroke_color);
        painter.add(Shape::closed_line(path, stroke));

        // Paint the rotated number
        let number_pos = number_center - (rotation * (number_size / 2.0));
        painter.add(TextShape {
            galley: number_galley,
            pos: number_pos,
            angle: std::f32::consts::PI,
            underline: Stroke::default(),
            fallback_color: Color32::WHITE,
            opacity_factor: 1.0,
            override_text_color: None,
            
        });

        // The name runs down from the top-left; the rotated day number sits in
        // the bottom-right, so the third row has to stop short of it. That is
        // the whole reason `fit_text_rows` takes a width per row.
        //
        // What was here had no "the first row is full" flag at all: once a word
        // failed to fit, later *shorter* words were still appended to row one,
        // so the name printed with its words out of order — and row two was
        // built with `push_str(word)` and no separator, so the rest of it
        // printed run together as one string.
        let color = Color32::from_gray(150);

        let left = rect.left() + margin + 7.0;
        let right = rect.right() - margin;
        let number_left = number_center.x - rotated_size.x / 2.0 - margin;

        let top = rect.top() + margin + 1.0;
        let count = rows_between(top, rect.bottom() - margin);
        // A row that reaches down into the number's band stops short of it;
        // the ones above it get the whole card.
        let number_top = number_center.y - rotated_size.y / 2.0 - 2.0;
        let widths: Vec<f32> = (0..count)
            .map(|index| {
                let row_bottom = top + (index as f32 + 1.0) * CARD_LINE_HEIGHT;
                if row_bottom > number_top { (number_left - left).max(0.0) } else { right - left }
            })
            .collect();

        let rows = fit_text_rows(ui, self.text, &text_font, &widths);
        paint_rows(&painter, &rows, &vec![left; count], Pos2::new(left, top), &text_font, color);



        // 1. Compute your external position
        let hourmark_pos = Pos2::new(
            rect.center().x  - 7.0,
            rect.bottom() - 7.0,
        );

        // 2. Prepare background and text layout
        let hour_label = self.hour.to_string();
        let hour_font = FontId {
            size: 10.0,
            family: FontFamily::Name("space".into()),
        };
        let hour_size = ui.fonts_mut(|f| f.layout_no_wrap(hour_label.clone(), hour_font.clone(), color).size());
        let hour_padding = 3.0;

        let bg_rect = Rect::from_min_size(
            hourmark_pos - vec2(hour_padding, hour_padding / 2.0),
            hour_size + vec2(hour_padding * 2.0, hour_padding),
        );

        // 3. Create a painter with an **infinite clip rect**
        let unclipped_painter = ui.painter().with_clip_rect(Rect::EVERYTHING);

        // 4. Draw outside the original bounds safely
        unclipped_painter.rect_filled(bg_rect, 6.0, Color32::from_black_alpha(40));
        unclipped_painter.rect_stroke(bg_rect, 6.0, Stroke::new(0.1, Color32::from_white_alpha(120)), StrokeKind::Middle);
        unclipped_painter.text(hourmark_pos, Align2::LEFT_TOP, hour_label, hour_font, Color32::from_white_alpha(150));


        if let Some(hour) = self.top_hour {
            let hourmark_pos = Pos2::new(
                rect.center().x - 59.0,
                rect.top() - 11.5,
            );

            let hour_label = hour.to_string();
            let hour_font = FontId {
                size: 10.0,
                family: FontFamily::Name("space".into()),
            };
            let hour_size = ui.fonts_mut(|f| f.layout_no_wrap(hour_label.clone(), hour_font.clone(), color).size());
            let hour_padding = 3.0;

            let bg_rect = Rect::from_min_size(
                hourmark_pos - vec2(hour_padding, hour_padding / 2.0),
                hour_size + vec2(hour_padding * 2.0, hour_padding),
            );

            let unclipped_painter = ui.painter().with_clip_rect(Rect::EVERYTHING);

            unclipped_painter.rect_filled(bg_rect, 6.0, Color32::from_black_alpha(40));
            unclipped_painter.rect_stroke(bg_rect, 6.0, Stroke::new(0.1, Color32::from_white_alpha(120)), StrokeKind::Middle);
            unclipped_painter.text(hourmark_pos, Align2::LEFT_TOP, hour_label, hour_font, Color32::from_white_alpha(150));
        }


        response
    }
}


pub struct ButtonHeaderRotated<'a> {
    pub number: &'a str,
    pub text: &'a str,
    pub is_strong: bool,
    pub hour: &'a str,
    pub top_hour: Option<&'a str>,
    pub color: Color32,
}

impl<'a> ButtonHeaderRotated<'a> {
    pub fn new(
        number: &'a str,
        text: &'a str,
        is_strong: bool,
        hour: &'a str,
        top_hour: Option<&'a str>,
        color: Color32,
    ) -> Self {
        Self { number, text, is_strong, hour, top_hour, color }
    }
}

impl<'a> egui::Widget for ButtonHeaderRotated<'a> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let desired_size = vec2(ui.available_width(), 60.0);
        let (rect, response) = ui.allocate_exact_size(desired_size, Sense::hover());

        let painter = ui.painter_at(rect);

        let text_font = card_text_font();

        let margin = 7.0;

        // Measure number galley (rotated)
        let number_galley = ui.fonts_mut(|f| {
            let color = if self.is_strong {
                ui.style().visuals.strong_text_color()
            } else {
                ui.style().visuals.text_color()
            };

            // let font_id = FontSelection::Default.resolve(ui.style());
            let font_id = FontId { size: 16.0, family: FontFamily::Name("anton".into()) };
            f.layout_no_wrap(self.number.to_string(), font_id, color)
        });

        let number_size = number_galley.size();
        let rotation = egui::emath::Rot2::from_angle(std::f32::consts::PI); // 180 degrees

        // Position rotated number at bottom right
        let rotated_bb = Rect::from_center_size(Pos2::ZERO, number_size).rotate_bb(rotation);
        let rotated_size = rotated_bb.size();

        let number_center = Pos2::new(
            rect.right() - margin - rotated_size.x / 2.0,
            rect.bottom() - margin - rotated_size.y / 2.0 + 4.0,
        );

        // Calculate surrounding area for the path
        let path = {
            let mut path = Vec::new();

            //rounded
            let radius = 8.0;
            let segments = 5; // More segments = smoother curve

            // Arc center is inset from bottom-left corner
            let arc_center = Pos2::new(rect.left() + radius, rect.bottom() - radius);

            // Start of arc (horizontal line end)
            path.push(Pos2::new(number_center.x - rotated_size.x / 2.0 - margin, rect.bottom()));
            path.push(Pos2::new(arc_center.x, rect.bottom()));

            // Bottom-left arc: 90° curve from bottom to left
            for i in 0..=segments {
                let t = i as f32 / segments as f32;
                let angle = std::f32::consts::FRAC_PI_2 * (1.0 - t); // From 90° to 0°
                let x = arc_center.x - radius * angle.cos();
                let y = arc_center.y + radius * angle.sin();
                path.push(Pos2::new(x, y));
            }
            //

            path.push(Pos2::new(rect.left(), rect.top()));
            path.push(Pos2::new(rect.right(), rect.top()));
            path.push(Pos2::new(rect.right(), number_center.y - rotated_size.y / 2.0 - margin));
            path.push(Pos2::new(number_center.x - rotated_size.x / 2.0 - margin, number_center.y - rotated_size.y / 2.0 - margin));
            path.push(Pos2::new(number_center.x - rotated_size.x / 2.0 - margin, rect.bottom()));
            path
        };

        // Background fill
        // let bg_color = ui.visuals().widgets.hovered.bg_fill;
        let bg_color = self.color;

        painter.add(Shape::convex_polygon(path.clone(), bg_color, Stroke::NONE));

        // Outline stroke
        let stroke_color = ui.visuals().widgets.noninteractive.bg_stroke.color;
        let stroke = Stroke::new(1.0, stroke_color);
        painter.add(Shape::closed_line(path, stroke));

        // Paint the rotated number
        let number_pos = number_center - (rotation * (number_size / 2.0));
        painter.add(TextShape {
            galley: number_galley,
            pos: number_pos,
            angle: std::f32::consts::PI,
            underline: Stroke::default(),
            fallback_color: Color32::WHITE,
            opacity_factor: 1.0,
            override_text_color: None,
            
        });

        // The name runs down from the top-left; the rotated day number sits in
        // the bottom-right, so the third row has to stop short of it. That is
        // the whole reason `fit_text_rows` takes a width per row.
        //
        // What was here had no "the first row is full" flag at all: once a word
        // failed to fit, later *shorter* words were still appended to row one,
        // so the name printed with its words out of order — and row two was
        // built with `push_str(word)` and no separator, so the rest of it
        // printed run together as one string.
        let color = Color32::from_gray(150);

        let left = rect.left() + margin + 7.0;
        let right = rect.right() - margin;
        let number_left = number_center.x - rotated_size.x / 2.0 - margin;

        let top = rect.top() + margin + 1.0;
        let count = rows_between(top, rect.bottom() - margin);
        // A row that reaches down into the number's band stops short of it;
        // the ones above it get the whole card.
        let number_top = number_center.y - rotated_size.y / 2.0 - 2.0;
        let widths: Vec<f32> = (0..count)
            .map(|index| {
                let row_bottom = top + (index as f32 + 1.0) * CARD_LINE_HEIGHT;
                if row_bottom > number_top { (number_left - left).max(0.0) } else { right - left }
            })
            .collect();

        let rows = fit_text_rows(ui, self.text, &text_font, &widths);
        paint_rows(&painter, &rows, &vec![left; count], Pos2::new(left, top), &text_font, color);


        let button_size = vec2(30.0, 18.0);
        let button_pos = Pos2::new(rect.left() + margin, rect.bottom() - margin - button_size.y);
        let button_rect = Rect::from_min_size(button_pos, button_size);

        ui.scope_builder(egui::UiBuilder::new().max_rect(button_rect), |ui| {
            let painter = ui.painter();

            // Draw the rounded frame (border)
            let rounding = 4.0; // Radius for the corners
            let stroke = Stroke::new(1.0, Color32::from_white_alpha(100)); // Border thickness and color
            let fill = Color32::from_white_alpha(20); // Optional background fill (transparent)

            painter.rect(
                button_rect,
                rounding,
                fill,
                stroke,
                StrokeKind::Outside
            );

            // Draw the text centered
            let text_pos = button_rect.center();
            painter.text(
                text_pos,
                egui::Align2::CENTER_CENTER,
                "…",
                FontId { size: 25.0, family: FontFamily::Monospace },
                Color32::from_white_alpha(180),
            );
        });

        
        // 1. Compute your external position
        let hourmark_pos = Pos2::new(
            rect.center().x  - 7.0,
            rect.bottom() - 7.0,
        );

        // 2. Prepare background and text layout
        let hour_label = self.hour.to_string();
        let hour_font = FontId {
            size: 10.0,
            family: FontFamily::Name("space".into()),
        };
        let hour_size = ui.fonts_mut(|f| f.layout_no_wrap(hour_label.clone(), hour_font.clone(), color).size());
        let hour_padding = 3.0;

        let bg_rect = Rect::from_min_size(
            hourmark_pos - vec2(hour_padding, hour_padding / 2.0),
            hour_size + vec2(hour_padding * 2.0, hour_padding),
        );

        // 3. Create a painter with an **infinite clip rect**
        let unclipped_painter = ui.painter().with_clip_rect(Rect::EVERYTHING);

        // 4. Draw outside the original bounds safely
        unclipped_painter.rect_filled(bg_rect, 6.0, Color32::from_black_alpha(40));
        unclipped_painter.rect_stroke(bg_rect, 6.0, Stroke::new(0.1, Color32::from_white_alpha(120)), StrokeKind::Middle);
        unclipped_painter.text(hourmark_pos, Align2::LEFT_TOP, hour_label, hour_font, Color32::from_white_alpha(150));


        if let Some(hour) = self.top_hour {
            let hourmark_pos = Pos2::new(
                rect.center().x - 59.0,
                rect.top() - 11.5,
            );

            let hour_label = hour.to_string();
            let hour_font = FontId {
                size: 10.0,
                family: FontFamily::Name("space".into()),
            };
            let hour_size = ui.fonts_mut(|f| f.layout_no_wrap(hour_label.clone(), hour_font.clone(), color).size());
            let hour_padding = 3.0;

            let bg_rect = Rect::from_min_size(
                hourmark_pos - vec2(hour_padding, hour_padding / 2.0),
                hour_size + vec2(hour_padding * 2.0, hour_padding),
            );

            let unclipped_painter = ui.painter().with_clip_rect(Rect::EVERYTHING);

            unclipped_painter.rect_filled(bg_rect, 6.0, Color32::from_black_alpha(40));
            unclipped_painter.rect_stroke(bg_rect, 6.0, Stroke::new(0.1, Color32::from_white_alpha(120)), StrokeKind::Middle);
            unclipped_painter.text(hourmark_pos, Align2::LEFT_TOP, hour_label, hour_font, Color32::from_white_alpha(150));
        }


        response
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// Ten units a character, so a width of 100 is exactly ten characters and
    /// the expectations below can be read off by counting.
    fn fit(text: &str, widths: &[f32]) -> Vec<String> {
        fit_rows_by(text, &mut |_| 10.0, widths)
    }

    #[test]
    fn words_wrap_and_stay_in_order() {
        // The bug two of the four copies had: with no "row one is full" flag,
        // a word that didn't fit was skipped and later shorter words were still
        // appended to row one, so the name printed out of order.
        assert_eq!(
            fit("alpha bravo charlie x", &[100.0, 100.0, 100.0]),
            vec!["alpha", "bravo", "charlie x"]
        );
    }

    #[test]
    fn every_row_is_measured_not_just_the_first() {
        // The bug all four shared: the remainder was poured into row two
        // without measuring it, so it ran off the side of the card.
        let rows = fit("one two three four five six", &[100.0, 100.0, 100.0]);
        for row in &rows {
            assert!(row.chars().count() <= 10, "{row:?} overflows its row");
        }
    }

    #[test]
    fn a_row_that_is_narrower_is_given_less() {
        // The day number shortens the first row; the rows below get the whole
        // card. Passing a width per row is the whole point of the helper.
        assert_eq!(fit("aaa bbbb cccc", &[30.0, 90.0]), vec!["aaa", "bbbb cccc"]);
    }

    #[test]
    fn a_word_too_long_for_any_row_is_broken_not_dropped() {
        // "Supercalifragilisticexpialidocious" has nowhere to break. The old
        // code left row one empty and pushed the whole thing into row two,
        // where it was never measured and overflowed the card.
        let rows = fit("Supercalifragilisticexpialidocious", &[100.0, 100.0]);
        assert_eq!(rows[0], "Supercalif", "a full row of it");
        // The last row is nine characters and the mark, not ten and an
        // overhang: the ellipsis is paid for out of the row's own width.
        assert_eq!(rows[1], "ragilisti…");
        assert!(rows[1].chars().count() <= 10, "{:?} overflows", rows[1]);
    }

    #[test]
    fn what_did_not_fit_is_marked_with_an_ellipsis() {
        // Silently dropping the tail is what made a cut-off name look like the
        // whole name.
        let rows = fit("alpha bravo charlie delta echo", &[100.0, 100.0]);
        assert_eq!(rows.len(), 2);
        assert!(rows[1].ends_with(ELLIPSIS), "{:?}", rows[1]);
        // ...and the ellipsis is paid for out of the row, not added past its end.
        assert!(rows[1].chars().count() <= 10, "{:?} overflows", rows[1]);
    }

    #[test]
    fn a_name_that_fits_is_left_exactly_alone() {
        assert_eq!(fit("Dentist", &[100.0, 100.0]), vec!["Dentist"]);
        // Filling a row to the last character is not "left over".
        let exact = fit("abcde fghi", &[100.0, 100.0]);
        assert_eq!(exact, vec!["abcde fghi"]);
    }

    #[test]
    fn the_ellipsis_never_hangs_off_a_space() {
        // Trimming back to make room can leave a trailing space; "half a …"
        // reads as a typo rather than as a truncation.
        let rows = fit("aaaa bb cccccccccc", &[100.0, 60.0]);
        let last = rows.last().unwrap();
        assert!(!last.contains(" \u{2026}"), "{last:?}");
    }

    #[test]
    fn degenerate_inputs_terminate() {
        // A row too narrow for even one character must still make progress
        // rather than asking for the same word forever.
        let rows = fit("wide", &[1.0, 1.0, 1.0]);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|row| !row.is_empty()));

        assert!(fit("", &[100.0]).is_empty());
        assert!(fit("   ", &[100.0]).is_empty());
        assert!(fit("anything", &[]).is_empty());
    }

    #[test]
    fn multibyte_names_are_cut_on_character_boundaries() {
        // Byte-slicing a broken word would panic in the middle of a codepoint.
        let rows = fit("日本語のとても長い名前です", &[50.0, 50.0]);
        assert_eq!(rows[0].chars().count(), 5);
        assert!(rows[1].ends_with(ELLIPSIS));
    }
}
