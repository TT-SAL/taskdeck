use std::{collections::HashMap, error::Error, fs, path::PathBuf, process::{Command, exit}, sync::{Arc, atomic::Ordering, mpsc::{Receiver, Sender}}, time::Instant};

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Timelike, Weekday};
use egui::{self, Align, Button, Color32, ColorImage, ComboBox, Context, CornerRadius, Event, FontData, FontDefinitions, FontFamily, FontId, Grid, Key, Label, Layout, Margin, PointerButton, Pos2, Rect, RichText, Stroke, StrokeKind, TextureHandle, Ui, Vec2, ViewportCommand, pos2, vec2};
use image::{ImageBuffer, Rgba};
use winit::event_loop::EventLoopProxy;

use crate::{archive::{self, ArchiveKey, ArchiveLog, Archived, KindFilter, Outcome, OutcomeFilter}, board::{self, Board}, sync, calendarwidgets, color::{self, ColorScheme}, initialization::{DESIGN_WIDTH_POINTS, FRAME_CAP_DEFAULT, FRAME_CAP_MAX, FRAME_CAP_MIN, FRAME_CAP_UNCAPPED, UI_SCALE_AUTO, UI_SCALE_MAX, UI_SCALE_MIN, clamp_frame_cap, clamp_ui_scale_percent}, paths::AppDirs, phone, planner, subscriptions, utilities::{self, next_three_weekdays, resolve_colorscheme}, tasks::{self, Active}, weather::{self, WeatherService}};

/// The calendar's column headings. The same Monday-first list a `Recurrence`
/// bit indexes into, so there is one place a weekday is named.
const WEEK_DAYS: [&str; 7] = tasks::WEEKDAY_NAMES;

/// Labels for an undated task's **horizon** and a dated one's **severity**,
/// and the order a combo presents the horizons in. Defined next to the model
/// they describe (`tasks`), because the phone view names them too.
const HORIZON: [&str; 4] = tasks::HORIZON_LABELS;
const HORIZON_DISPLAY_ORDER: [u8; 4] = tasks::HORIZON_DISPLAY_ORDER;
const IMPORTANCE: [&str; 5] = tasks::IMPORTANCE_LABELS;

/// How long a restarted phone server keeps trying to bind its port while the
/// old socket closes, and how often it tries.
const PHONE_RESTART_WINDOW: std::time::Duration = std::time::Duration::from_secs(4);
const PHONE_RETRY_EVERY: std::time::Duration = std::time::Duration::from_millis(150);
/// How long the addresses shown in Settings are trusted before being looked up
/// again — a laptop that changes Wi-Fi should not show yesterday's address.
const PHONE_ADDRESS_TTL: std::time::Duration = std::time::Duration::from_secs(20);
/// Side of the QR code in Settings, and the quiet zone around it in modules.
const PHONE_QR_SIZE: f32 = 176.0;
const PHONE_QR_QUIET: usize = 3;

/// How much accumulated error text the error window will hold before it stops
/// appending and says so. See `show_error`.
const ERROR_TEXT_CAP: usize = 1200;
const ERROR_TEXT_MORE: &str = "\n\n(more errors followed)";
/// Tallest the error window's text area gets before it scrolls.
const ERROR_TEXT_MAX_HEIGHT: f32 = 520.0;

/// The masthead's "behind" figure, in the colour the phone view uses for today.
const PLANNER_BEHIND_COLOR: Color32 = Color32::from_rgb(240, 195, 107);

/* ─────────────────────────── Day planner layout ─────────────────────────── */

/// Height of one hour on the planner timeline.
///
/// The trade the whole window turns on: taller hours are easier to aim a
/// 15-minute block at, shorter ones put more of the day on screen at once. At
/// 48 the snap step is still a dozen points — a comfortable target — and a
/// full day is 1152, which fits inside the body of the window on an ordinary
/// screen with only the small hours left to scroll to.
const PLANNER_HOUR_HEIGHT: f32 = 48.0;
/// Width of the hour-label gutter down the left of the timeline.
const PLANNER_GUTTER_WIDTH: f32 = 52.0;
/// Width of the backlog tray.
const PLANNER_TRAY_WIDTH: f32 = 268.0;
/// Grab area at the bottom of a block for resizing it. Generous: this is the
/// only way to change a block's length with the pointer, and at eight points it
/// was under six pixels on a scaled-down window — a target you had to hunt for.
const PLANNER_RESIZE_HANDLE: f32 = 14.0;
/// A block at least this tall puts its name on a second line under the times.
/// Below it the name shares the row, right-aligned. Sized for the time line plus
/// a `PLANNER_NAME_SIZE` name plus the block's own vertical margins.
const PLANNER_TWO_LINE_BLOCK: f32 = 46.0;
/// Hour brought into view when opening a day that isn't today.
const PLANNER_DEFAULT_SCROLL_HOUR: f32 = 7.0;
/// Height of the footer. Reserved whether or not anything is selected, so
/// selecting doesn't shift the timeline under the pointer; it carries the day's
/// gesture hints when the selection is empty. Tall enough for the combo boxes,
/// which are the deepest things the row ever holds.
const PLANNER_INSPECTOR_HEIGHT: f32 = 46.0;
/// Point size of the weekday in the masthead — the day popup's headline size,
/// kept because it is the one thing worth carrying over from that window.
const PLANNER_WEEKDAY_SIZE: f32 = 50.0;
/// Gap between the masthead's outermost controls and the window edge. The
/// window frame's own margin is small enough that the day stepper and the ✕ sat
/// in the corners; this is what keeps them off it.
const PLANNER_EDGE_MARGIN: f32 = 14.0;
/// How much of the window the planner leaves showing around itself. Wider than
/// it is tall: a strip of calendar down each side says what is behind the
/// window, while every point of height is another few minutes of the day.
const PLANNER_WINDOW_INSET: Vec2 = Vec2::new(160.0, 64.0);

/* ─────────────────────────── Planner type scale ───────────────────────────
 *
 * Three sizes, used everywhere in the planner instead of a literal per call
 * site. The app sets Body at 18 points and Button at 22 (see `set_styles`)
 * because it is meant to be read from across the room; the planner had drifted
 * to 11–14, which is a different application's typography and looked it next to
 * a 22-point combo box. These sit under the body size — a timeline is denser
 * than a task card, and a 15-minute block has to fit its own name — without
 * dropping into the footnote range.
 */

/// Names: block titles, tray card titles, the selected item in the footer.
const PLANNER_NAME_SIZE: f32 = 17.0;
/// Everything that qualifies a name: times, deadlines, field labels, hints.
const PLANNER_META_SIZE: f32 = 15.0;
/// Section headings and the second line of a tray card.
const PLANNER_FINE_SIZE: f32 = 13.0;
/// Step the automatic UI scale quantizes points-per-pixel to. See
/// `apply_ui_scale` — a quarter step keeps the layout close to the size that
/// fits while giving text a much better chance of a whole-pixel height.
const PPP_QUANTUM: f32 = 0.25;

/// Severity seeded onto a task the moment the due editor gives it its first
/// deadline. Mid-scale: a dated task is scored by how bad missing the date is,
/// the user hasn't said yet, and the footer is right there to adjust it.
const PLANNER_NEW_TASK_IMPORTANCE: u8 = 2;

/// Horizon given to a task created on the timeline or in the quick-add:
/// "within a week", the middle of the road. Matches the New Task dialog's
/// default, so where a task was typed doesn't change what it is.
const PLANNER_NEW_TASK_HORIZON: u8 = 1;

/// Text size in the hover tip that names a day's items in full.
const CELL_TIP_SIZE: f32 = 14.0;

/// How many of a day's items the hover tip will name before it starts counting
/// them instead. The tip is a floating popup with the whole window to grow
/// into, so this is not about space — it is a ceiling so that a pathological
/// day cannot produce a tip taller than the screen.
const CELL_TIP_MAX_ROWS: usize = 14;

/* ──────────────────────────────── The archive ─────────────────────────────
 *
 * A ledger, and deliberately built like the planner rather than like the grid
 * it replaces: a masthead that says what the rows add up to, a body you read,
 * and a footer that acts on whatever is selected. The old window was a fixed
 * 500×800 three-column table in hardcoded Dracula colours with a "Show more"
 * button under it — it could tell you that a thing had happened and the date it
 * happened on, and nothing else at all.
 */

/// How much of the window the archive leaves showing around itself. Narrower
/// than the planner: a ledger is a column of lines, and a very wide one is
/// harder to read, not easier.
const ARCHIVE_WINDOW_INSET: Vec2 = Vec2::new(420.0, 96.0);
/// Bounds on its width. The floor is what the filter row needs before its two
/// ends meet; the ceiling is where a line of text stops being a line.
const ARCHIVE_MIN_WIDTH: f32 = 620.0;
const ARCHIVE_MAX_WIDTH: f32 = 1080.0;
/// Width of the date spine down the left of the ledger — "15 Aug" over
/// "Wed 21:04" in the mono face, which is what sets the number.
const ARCHIVE_DATE_COLUMN: f32 = 92.0;
/// Height of the footer. Reserved whether or not a row is selected, so
/// selecting one doesn't shift the ledger under the pointer — the same bargain
/// the planner's inspector makes.
const ARCHIVE_FOOTER_HEIGHT: f32 = 62.0;
/// The archive's type scale, one step under the planner's: this is a document
/// you read at the desk, not a timeline you aim a pointer at.
const ARCHIVE_NAME_SIZE: f32 = 16.0;
const ARCHIVE_META_SIZE: f32 = 14.0;
const ARCHIVE_FINE_SIZE: f32 = 12.0;
/// Month headings, in the same face the planner's tray headings use.
const ARCHIVE_MONTH_SIZE: f32 = 13.0;

/// Everything the archive window remembers between frames.
///
/// One struct rather than the four loose fields this replaces (`archive`,
/// `offset`, `display_archive_flag`, and a confirmation flag it would otherwise
/// have needed): `CODE_REVIEW.md` D6 asks for the modal booleans to stop
/// multiplying, and the honest way to do that is for a screen to own its own
/// state rather than scattering it across `TaskApp`.
#[derive(Default)]
struct ArchiveView {
    open: bool,
    filter: archive::Filter,
    /// The row the footer is editing.
    selected: Option<ArchiveKey>,
    /// Row awaiting a "forget this permanently" confirmation. Forgetting is the
    /// only genuinely irreversible thing in the app, so it asks — restoring
    /// does not, because it can simply be done again.
    confirm_forget: Option<ArchiveKey>,
    /// Set for the one frame after `/` is pressed, to put the caret in the
    /// search field. One frame, not every frame — see `planner_naming_focus`
    /// for what re-requesting focus every frame does to a text field.
    ///
    /// Deliberately **not** set on open. The archive is mostly read, not
    /// searched, and a field that grabs the keyboard the moment the window
    /// appears takes every letter shortcut with it — `A` would type an `a`
    /// instead of closing the window it just opened.
    focus_search: bool,
}

/* ─────────────────────────── The settings sheet ───────────────────────────
 *
 * One column of labelled rows in named sections, at a width the sheet chooses
 * rather than one it is cut off at. What was here before was a single-column
 * `Grid` used as a spacer — every row a `horizontal_centered` with a pair of
 * `end_row()`s after it for padding — inside a window pinned to 400×300 with
 * far more than that in it, so nothing lined up with anything and the last
 * rows fell out of the bottom.
 */

/// Width of the settings window, and of the colour-scheme manager beside it.
const SETTINGS_WIDTH: f32 = 660.0;
/// Width of the label column. Every section shares it, so the controls line up
/// down the whole sheet rather than per section.
const SETTINGS_LABEL_COLUMN: f32 = 200.0;
/// Width of a combo box or slider in the control column.
const SETTINGS_CONTROL_WIDTH: f32 = 240.0;
/// Height at which the body starts scrolling instead of growing.
const SETTINGS_MAX_BODY_HEIGHT: f32 = 660.0;
/// Row labels and control text. Under the 18-point body size: a settings sheet
/// is a dense form, not something read from across the room.
const SETTINGS_LABEL_SIZE: f32 = 16.0;
/// Units, ranges, and the notes that qualify a control.
const SETTINGS_FINE_SIZE: f32 = 13.0;
/// Section headings, in the same face the planner's tray headings use.
const SETTINGS_SECTION_SIZE: f32 = 13.0;

/* ─────────────────────────── The weather column ───────────────────────────
 *
 * The forecast is a 4×3 grid of two-hour slots per day, and the notepad sits
 * under it in the same column, so both are measured from the same numbers.
 */

/// Size of one forecast cell. Fixed, and painted rather than laid out — see
/// `display_stuff`.
const WEATHER_CELL: Vec2 = Vec2::new(80.0, 78.0);
/// Gap between cells, in both directions.
const WEATHER_CELL_GAP: f32 = 10.0;
/// Columns of cells in a day's grid.
const WEATHER_COLUMNS: f32 = 4.0;
/// Side of the sky icon inside a cell.
const WEATHER_ICON: f32 = 48.0;
/// Width of a day's grid, and therefore of the column the notepad shares with
/// it. Derived, so the notepad cannot drift wider than the forecast above it —
/// which is exactly what it had done.
const WEATHER_GRID_WIDTH: f32 =
    WEATHER_COLUMNS * WEATHER_CELL.x + (WEATHER_COLUMNS - 1.0) * WEATHER_CELL_GAP;

/* The column's two vertical gaps.
 *
 * These were both 75 points, and looked like nothing of the sort: the old cell
 * was a `Frame` laid out **bottom-up**, which anchored its content to the
 * bottom of an available rect the overflowing column had already exhausted, so
 * each grid was painted the better part of a cell-height *above* where the
 * layout had put it and swallowed most of the space above it. The numbers were
 * tuned against that, so making the cells honest (painting them at the rect
 * they are allocated) left all of it showing at once and pushed every grid down
 * the column. These are what the old arrangement actually measured. */

/// Between a weekday and its grid.
const WEATHER_LABEL_GAP: f32 = 6.0;
/// Between one day's grid and the next day's weekday.
const WEATHER_DAY_GAP: f32 = 16.0;

/// The weekday over a day's grid, centred on the grid rather than pushed into
/// place by an `add_space` guessed per call site — there were two different
/// guesses for what was supposed to be the same position.
fn weather_day_label(ui: &mut Ui, text: &str) {
    ui.scope(|ui| {
        ui.set_width(WEATHER_GRID_WIDTH);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(text)
                    .size(14.0)
                    .color(Color32::from_white_alpha(165)),
            );
        });
    });
}

/* ─────────────────────────── The notepad ─────────────────────────── */

/// Padding between the card's edge and the writing area.
const NOTEPAD_PADDING: f32 = 10.0;
/// Width of the writing area: the forecast grid's width, less the card's own
/// margins and its stroke, so the card ends exactly where the cells above it
/// do. It used to be a round number that happened to be wider, and the card
/// visibly overhung the column.
const NOTEPAD_WIDTH: f32 = WEATHER_GRID_WIDTH - 2.0 * NOTEPAD_PADDING - 3.0;
/// Gap left under the card, so it doesn't sit on the bottom edge of the window.
const NOTEPAD_BOTTOM_MARGIN: f32 = 14.0;
/// Bounds on the card's height, which is otherwise the column's own remaining
/// space. The floor keeps a usable box on a short window; the ceiling stops the
/// notes becoming the largest thing in the app on a very tall one.
const NOTEPAD_MIN_HEIGHT: f32 = 160.0;
const NOTEPAD_MAX_HEIGHT: f32 = 520.0;
/// Size of the notes themselves. A note is read from the desk, not from across
/// the room, so it sits under the 19 points this used to be — which cost two
/// characters of line width for nothing.
const NOTEPAD_TEXT_SIZE: f32 = 17.0;
/// The card's own heading, and the unsaved marker beside it.
const NOTEPAD_HEADING_SIZE: f32 = 12.0;
/// What the card spends on its heading and margins, subtracted from the card's
/// height to get the writing area's.
const NOTEPAD_CHROME_HEIGHT: f32 = 46.0;

/// How much of the window the coordinate picker leaves showing around itself.
const MAP_WINDOW_INSET: f32 = 200.0;
/// Ceiling on the map's width. Past this the picture (1920×960) is being
/// magnified rather than shown.
const MAP_MAX_WIDTH: f32 = 1500.0;
/// Floor on it: the width the footer row needs before its two ends meet.
const MAP_MIN_WIDTH: f32 = 720.0;

/// Width of the scheme list in the colour-scheme manager.
const SCHEME_LIST_WIDTH: f32 = 330.0;
/// Height of one row of that list.
const SCHEME_ROW_HEIGHT: f32 = 30.0;
/// Gap between rows, set explicitly so a list can be made exactly as tall as
/// the rows it holds. Sizing one by `rows × height` and letting egui's default
/// spacing pile up underneath is how the last scheme ended up below the fold of
/// a list nobody expects to scroll.
const SCHEME_ROW_GAP: f32 = 4.0;
/// Height one row costs a list, gap included.
const SCHEME_ROW_PITCH: f32 = SCHEME_ROW_HEIGHT + SCHEME_ROW_GAP;
/// Side of one palette swatch in a scheme row.
const SCHEME_SWATCH: f32 = 17.0;
/// Width of the scheme editor window.
const SCHEME_EDITOR_WIDTH: f32 = 460.0;
/// Side of one editable swatch in that window.
const SCHEME_EDIT_SWATCH: f32 = 46.0;
/// Height of its "on the calendar" preview strip.
const SCHEME_PREVIEW_HEIGHT: f32 = 44.0;

/// One editable swatch in the scheme editor.
///
/// egui's colour button is the swatch itself rather than something painted
/// under it, so what is shown is the colour over a checkerboard — alpha read as
/// alpha, which is what you need while editing and exactly what you cannot
/// judge the calendar from. The preview strip below the row answers that half.
///
/// A click opens the picker and a drag swaps two swatches: egui resolves click
/// and drag targets separately, so the colour button (which senses clicks only)
/// takes the click while the drag falls through to the rect underneath it.
fn colorscheme_swatch(
    ui: &mut Ui,
    color: &mut [u8; 4],
    index: usize,
    dragged: &mut Option<usize>,
    swap_with: &mut Option<usize>,
) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::splat(SCHEME_EDIT_SWATCH), egui::Sense::click_and_drag());

    if response.drag_started() {
        *dragged = Some(index);
    }
    if let Some(from) = *dragged {
        if from != index && response.hovered() {
            *swap_with = Some(index);
        }
    }

    // A ring around the swatch being carried, so a drag looks like one.
    if *dragged == Some(index) {
        ui.painter().rect_stroke(
            rect.expand(3.0),
            CornerRadius::same(10),
            Stroke::new(1.5, Color32::from_white_alpha(160)),
            StrokeKind::Outside,
        );
    }

    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        // Makes egui's button exactly the swatch, rather than a small chip in
        // the corner of one.
        ui.spacing_mut().interact_size = rect.size();
        ui.color_edit_button_srgba_unmultiplied(color);
    });
}

/// A section title, in the face the planner's tray headings use.
fn settings_section_heading(ui: &mut Ui, title: &str) {
    ui.label(
        RichText::new(title)
            .font(FontId::new(SETTINGS_SECTION_SIZE, FontFamily::Name("space".into())))
            .color(Color32::from_white_alpha(125)),
    );
    ui.add_space(6.0);
}

/// A named group of settings rows.
///
/// The heading sits outside the grid and each section owns its own grid, so a
/// section can be moved or added without renumbering anything;
/// `SETTINGS_LABEL_COLUMN` is what keeps their columns agreeing.
fn settings_section(ui: &mut Ui, title: &str, id: &str, contents: impl FnOnce(&mut Ui)) {
    ui.add_space(12.0);
    settings_section_heading(ui, title);
    Grid::new(id)
        .num_columns(2)
        .min_col_width(SETTINGS_LABEL_COLUMN)
        .spacing([14.0, 10.0])
        .show(ui, contents);
}

/// One row: its label on the left, its controls on the right. An empty label
/// is a continuation of the row above it — a note, or the second half of a
/// control that needs two lines.
fn settings_row(ui: &mut Ui, label: &str, contents: impl FnOnce(&mut Ui)) {
    ui.label(RichText::new(label).size(SETTINGS_LABEL_SIZE));
    ui.horizontal(|ui| contents(ui));
    ui.end_row();
}

/// A note beside a control: units, a range, what the setting costs.
fn settings_note(ui: &mut Ui, text: impl Into<String>) {
    ui.label(
        RichText::new(text.into())
            .size(SETTINGS_FINE_SIZE)
            .color(Color32::from_white_alpha(120)),
    );
}

/// A button sized and set like everything else on the sheet.
fn settings_button(ui: &mut Ui, text: &str) -> egui::Response {
    ui.add(Button::new(RichText::new(text).size(SETTINGS_LABEL_SIZE)).min_size(vec2(0.0, 28.0)))
}

/// The timeline gesture and the maths that interprets it live in `planner`;
/// this alias keeps the call sites here short.
use planner::Drag as PlannerDrag;

/// One row on the planner timeline: an item, and where it sits on the day being
/// shown. Rebuilt each frame from `board.items` — the planner has no cached
/// model of its own, so it can't drift out of sync with the calendar.
struct PlannerEntry {
    id: u64,
    /// Which of the item's sessions this entry is, when it is a planned work
    /// block; `None` for an event's block and for due markers. Together with
    /// `id` this is the address a gesture edits.
    session: Option<usize>,
    name: String,
    color_id: usize,
    /// Whether this is a routine's generated block. It is drawn recessively and
    /// counted apart from planned work in the day summary — a routine is the
    /// backdrop a day is planned around, not something planned in it.
    routine: bool,
    placement: planner::Placement,
}

/// One block of a day that has already been accounted for: time an archived
/// item was given, drawn on the timeline behind everything still live.
///
/// This is what the lossless archive buys the planner. A day used to show only
/// what was still *coming* — tick the morning's work off and the morning went
/// blank, as though it had never been spent. The record keeps the sessions now,
/// so the day can keep them too.
///
/// Blocks only, never due markers: a ghost answers "what did this day go on",
/// and the deadline of something already dealt with is not part of that answer.
/// Ghosts carry no id and take no gestures — they are not in the entry list the
/// pointer handling reads, only in the lane packing, so a live block still lays
/// out beside one instead of on top of it.
struct PlannerGhost {
    name: String,
    outcome: Outcome,
    color_id: usize,
    placement: planner::Placement,
}

/// One card in the planner's tray: a task with no time set aside for it yet.
struct BacklogCard {
    id: u64,
    name: String,
    color_id: usize,
    deadline: Option<DateTime<Local>>,
    /// How long the task is estimated to take, if the user has said.
    duration: Option<u32>,
    /// Minutes already booked in sessions. With an estimate, the difference is
    /// what the card is worth when dropped — a "2h" card with an hour planned
    /// lands the remaining hour.
    planned: u32,
    /// This card is being renamed *and* the task has nothing on the shown day,
    /// so the card hosts the title editor. Exactly one place ever does.
    hosts_name_editor: bool,
}

/// Screen rectangle for a placement in its lane.
///
/// Both kinds share the lane width with whatever overlaps them, so a deadline
/// that lands inside a block sits beside it rather than on top of it: `lay_out`
/// already gives markers a column, and drawing them full-width regardless meant
/// a due marker painted straight over the title of the block it fell in. Where
/// nothing overlaps, the cluster is one column wide and the marker spans the
/// timeline exactly as before.
///
/// A marker is short rather than tall whatever its column, because a deadline is
/// a line in the day rather than a claim on it.
fn planner_entry_rect(
    placement: planner::Placement,
    lane: planner::Lane,
    lane_area: Rect,
    geometry: &planner::TimelineGeometry,
) -> Rect {
    let column_width = (lane_area.width() - 8.0) / lane.columns.max(1) as f32;
    let left = lane_area.left() + 4.0 + column_width * lane.column as f32;

    match placement {
        planner::Placement::Marker { at, .. } => {
            let top = geometry.y_for(at as f32);
            Rect::from_min_max(
                pos2(left + 1.0, top),
                pos2(left + column_width - 2.0, top + 18.0),
            )
        }
        planner::Placement::Block { start, minutes } => {
            let top = geometry.y_for(start as f32);
            let bottom = geometry.y_for((start + minutes as i32) as f32);
            Rect::from_min_max(
                pos2(left + 1.0, top + 1.0),
                pos2(left + column_width - 2.0, (bottom - 1.0).max(top + 12.0)),
            )
        }
    }
}

/// Side of one weekday toggle in the routine footer. Square, and big enough to
/// aim at on a scaled-down window without the row of seven crowding the buttons
/// beside it.
const PLANNER_WEEKDAY_TOGGLE: f32 = 26.0;

/// A routine's one knob: which days it falls on.
///
/// It sits in the same slot a dated task's *severity* and an undated task's
/// *horizon* occupy, because it is the same kind of thing — the single question
/// that kind of item asks. Returns whether the rule changed.
///
/// Seven toggles rather than a combo of presets: "Mon Wed Fri" is as ordinary
/// as "every day", and a preset list either omits it or grows a "Custom…" that
/// opens the toggles anyway. **All** is there because the case this whole
/// feature exists for — sleeping, eating, the commute — is daily, and the
/// create gesture deliberately starts from one day (see `create_planned_item`).
fn planner_weekday_row(ui: &mut Ui, rule: &mut tasks::Recurrence, shown_day: NaiveDate) -> bool {
    let mut changed = false;

    ui.label(RichText::new("Repeats:").size(PLANNER_META_SIZE))
        .on_hover_text("Leave every day off and it happens once, on this day");

    for (index, name) in tasks::WEEKDAY_NAMES.iter().enumerate() {
        let on = rule.includes(index as u32);
        // The initial only — seven three-letter names is most of the footer,
        // and the row is in weekday order, which is what actually reads it.
        let initial = name.chars().next().unwrap_or('?').to_string();
        let button = Button::new(
            RichText::new(initial)
                .size(PLANNER_META_SIZE)
                .color(if on {
                    Color32::WHITE
                } else {
                    Color32::from_white_alpha(110)
                }),
        )
        .min_size(Vec2::splat(PLANNER_WEEKDAY_TOGGLE))
        .corner_radius(CornerRadius::same(6))
        .selected(on);

        if ui.add(button).on_hover_text(*name).clicked() {
            // Turning the last one off is allowed: that is a one-off, not a
            // broken rule. It re-anchors to the day being shown, so unticking
            // the last weekday leaves the block where you are looking rather
            // than sending it back to wherever it was first drawn.
            rule.toggle(index as u32, shown_day);
            changed = true;
        }
    }

    if ui
        .add_enabled(
            rule.days != tasks::EVERY_DAY,
            Button::new(RichText::new("All").size(PLANNER_META_SIZE)),
        )
        .on_hover_text("Every day")
        .clicked()
    {
        rule.days = tasks::EVERY_DAY;
        changed = true;
    }
    if ui
        .add_enabled(
            rule.repeats(),
            Button::new(RichText::new("Once").size(PLANNER_META_SIZE)),
        )
        .on_hover_text("Just this day")
        .clicked()
    {
        rule.days = 0;
        rule.anchor = shown_day;
        changed = true;
    }

    ui.label(
        RichText::new(rule.summary())
            .font(FontId::new(PLANNER_FINE_SIZE, FontFamily::Name("space".into())))
            .color(Color32::from_white_alpha(160)),
    );

    changed
}

/// Draw one ghost: an hour of this day that has already been accounted for.
///
/// Deliberately an outline over a dark wash rather than a filled block. A live
/// block is a solid object you can pick up and move; a ghost is a record, and
/// making it look grabbable would be a lie the pointer immediately exposes —
/// it takes no gestures at all. Painted before the live entries, so anything
/// still on the day covers it rather than the other way round.
/// Draw one subscribed calendar's event on the day's timeline.
///
/// Deliberately unlike a block of this board's: no fill of its own, a dashed
/// outline and a bar down the left in the calendar's own colour. It carries no
/// response and no gesture, so nothing about it invites the drag or the tap
/// that would do nothing. The colour is the subscription's rather than the
/// scheme's, because it says *which calendar*, not how urgent.
fn paint_subscribed_event(ui: &Ui, rect: Rect, color: Color32, name: &str, location: &str, background: bool) {
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(6), Color32::from_black_alpha(60));
    let weight = if background { 0.45 } else { 0.7 };
    painter.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, color.gamma_multiply(weight)), StrokeKind::Inside);
    // The bar is what reads at a glance as "this one is not mine".
    painter.rect_filled(
        Rect::from_min_max(rect.left_top(), pos2(rect.left() + 3.0, rect.bottom())),
        CornerRadius::same(2),
        color,
    );
    // The block is as tall as the event is long, so the text uses that height
    // rather than being cut to one line: a course summary runs past a hundred
    // characters and the room is at the end of it.
    let font = FontId::new(PLANNER_FINE_SIZE, FontFamily::Monospace);
    let inner = rect.shrink2(vec2(9.0, 3.0));
    let line = ui.fonts_mut(|fonts| fonts.row_height(&font));
    let rows = ((inner.height() / line).floor() as usize).max(1);
    let widths = vec![inner.width(); rows];
    let wrapped = calendarwidgets::fit_text_rows(ui, name, &font, &widths);

    let clipped = painter.with_clip_rect(rect.intersect(ui.clip_rect()));
    let mut y = inner.top();
    for row in &wrapped {
        clipped.text(pos2(inner.left(), y), egui::Align2::LEFT_TOP, row, font.clone(), Color32::from_white_alpha(150));
        y += line;
    }
    // Where it is, on the last line the block has room for, in the calendar's
    // own colour so it reads as the answer to a different question.
    if !location.is_empty() && wrapped.len() < rows {
        clipped.text(pos2(inner.left(), y), egui::Align2::LEFT_TOP, location, font, color.gamma_multiply(0.95));
    }
}

fn paint_planner_ghost(ui: &Ui, ghost: &PlannerGhost, rect: Rect, palette: &[Color32; 6]) {
    let accent = accent_for(palette, ghost.color_id);
    let painter = ui.painter();

    painter.rect_filled(rect, CornerRadius::same(8), Color32::from_black_alpha(90));
    painter.rect_stroke(
        rect,
        CornerRadius::same(8),
        // Half the weight of a live block's outline, in a washed-out version of
        // the colour the item wore in life.
        Stroke::new(1.0, accent.gamma_multiply(0.55)),
        StrokeKind::Inside,
    );

    let clipped = painter.with_clip_rect(rect.intersect(ui.clip_rect()));
    let text = format!("{}  {}", ghost.outcome.glyph(), ghost.name);
    clipped.text(
        rect.shrink2(vec2(8.0, 4.0)).left_top(),
        egui::Align2::LEFT_TOP,
        text,
        FontId::new(PLANNER_FINE_SIZE, FontFamily::Monospace),
        Color32::from_white_alpha(match ghost.outcome {
            Outcome::Finished => 165,
            // A block you abandoned is fainter than one you finished: the time
            // was set aside and then not spent, which is a quieter fact.
            Outcome::Dropped => 115,
        }),
    );
}

/// Accent colour for an item of a given palette index.
///
/// The palette drives it, exactly as on the calendar — but the default scheme
/// is six fully transparent entries (`ColorScheme::default_scheme`), because on
/// the calendar the background photo is meant to show through. Anything drawn
/// as a solid object — a planner block, an archive row's mark — has to stay
/// visible, so a transparent entry falls back to a neutral highlight.
fn accent_for(palette: &[Color32; 6], color_id: usize) -> Color32 {
    let color = palette[color_id.min(5)];
    if color.a() < 24 {
        Color32::from_white_alpha(85)
    } else {
        color
    }
}

/* ─────────────────────────── The archive window ───────────────────────────
 *
 * Built as a free function over `(&ArchiveLog, &mut ArchiveView)`: see
 * `TaskApp::show_archive` for why, and for where the returned intent is
 * carried out.
 */

/// The one thing a frame of the archive window can ask for. Everything the
/// window's buttons do is deferred into this, so nothing mutates the log while
/// its rows are still being drawn from.
enum ArchiveAction {
    Close,
    Restore(ArchiveKey),
    Forget(ArchiveKey),
}

fn archive_window(
    ctx: &Context,
    log: &ArchiveLog,
    view: &mut ArchiveView,
    palette: [Color32; 6],
    owns_keys: bool,
) -> Option<ArchiveAction> {
    // Sized from the viewport like the planner, and for the same reason: the
    // viewport is a different number of points on every machine. The old
    // window was pinned at 500×800 whatever it was opened on.
    let viewport = ctx.viewport_rect();
    let width =
        (viewport.width() - ARCHIVE_WINDOW_INSET.x).clamp(ARCHIVE_MIN_WIDTH, ARCHIVE_MAX_WIDTH);
    let height = (viewport.height() - ARCHIVE_WINDOW_INSET.y).clamp(360.0, 1400.0);

    // Filtering is one pass over rows that are already in the order they are
    // read in, and it borrows them rather than copying — which is the whole
    // reason the log lives in memory. Searching, grouping and the summary line
    // are all impossible against a fifteen-row window onto a file.
    let rows: Vec<&Archived> = log
        .entries()
        .iter()
        .filter(|row| view.filter.admits(row))
        .collect();
    let summary = archive::summarize(rows.iter().copied());
    let months = archive::group_by_month(&rows);

    // Captured before the body runs: the confirmation clears its own state the
    // instant it is answered, and the Escape that answered it is still in this
    // frame's input — so asking afterwards whether one was open answers "no" on
    // exactly the frame where it matters, and the key falls through and closes
    // the window underneath. The planner learned this the same way.
    let confirm_owned_frame = view.confirm_forget.is_some();
    let mut action = None;

    egui::Window::new("archive")
        // No title bar: the masthead names the window better than a caption,
        // and it carries the ✕ the way the planner's does.
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .default_size(vec2(width, height))
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.set_height(height);

            archive_masthead(ui, &summary, &mut action);
            archive_filter_row(ui, view, rows.len(), log.entries().len(), log.unreadable());
            ui.separator();

            // Measured, not guessed: the masthead's height depends on font
            // metrics and the UI scale, and a constant that disagreed with
            // either would push the footer off the bottom of the window.
            let body_height = (ui.available_height() - ARCHIVE_FOOTER_HEIGHT - 12.0).max(120.0);
            archive_ledger(ui, &rows, &months, view, palette, body_height);

            ui.separator();
            archive_footer(ui, &rows, view, &mut action);
        });

    // Drawn after the window so it stacks on top of it.
    archive_forget_confirmation(ctx, &rows, view, &mut action);

    if owns_keys && !confirm_owned_frame {
        let typing = ctx.egui_wants_keyboard_input();

        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            // Escape in two steps, so a search you are half way through is not
            // something the window closes over: it clears whatever the filter
            // is doing first, and only leaves once there is nothing to take
            // back.
            if view.filter.is_open() {
                action = Some(ArchiveAction::Close);
            } else {
                view.filter = archive::Filter::default();
            }
        } else if !typing && ctx.input(|i| i.key_pressed(Key::A)) {
            // The key that opened it closes it again.
            action = Some(ArchiveAction::Close);
        } else if !typing && ctx.input(|i| i.key_pressed(Key::Slash)) {
            view.focus_search = true;
        }
    }

    action
}

/// The title, and the one line that says what the rows below add up to.
fn archive_masthead(ui: &mut Ui, summary: &archive::Summary, action: &mut Option<ArchiveAction>) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.vertical(|ui| {
            ui.add(
                Label::new(
                    RichText::new("ARCHIVE")
                        .font(FontId::new(34.0, FontFamily::Name("anton".into()))),
                )
                .selectable(false),
            );
            ui.add_space(-6.0);
            // The figure the old window could never have shown: a fifteen-row
            // page cannot tell you how many deadlines you have met.
            ui.label(
                RichText::new(summary.headline())
                    .size(ARCHIVE_META_SIZE)
                    .color(Color32::from_white_alpha(190)),
            );
        });

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(14.0);
            if ui
                .add(
                    Button::new(RichText::new("✕").size(ARCHIVE_META_SIZE))
                        .min_size(vec2(30.0, 30.0))
                        .corner_radius(CornerRadius::same(8)),
                )
                .on_hover_text("Close  (Esc)")
                .clicked()
            {
                *action = Some(ArchiveAction::Close);
            }
        });
    });
    ui.add_space(6.0);
}

/// Search, and the two things worth narrowing by.
fn archive_filter_row(
    ui: &mut Ui,
    view: &mut ArchiveView,
    shown: usize,
    total: usize,
    unreadable: usize,
) {
    let label = |text: &str| {
        RichText::new(text.to_string())
            .font(FontId::new(ARCHIVE_FINE_SIZE, FontFamily::Name("space".into())))
            .color(Color32::from_white_alpha(120))
    };

    ui.horizontal(|ui| {
        ui.add_space(14.0);
        let search = ui.add(
            egui::TextEdit::singleline(&mut view.filter.query)
                .hint_text("search  (/)")
                .desired_width(200.0),
        );
        // One frame only. Re-requesting focus every frame is how a text field
        // becomes impossible to leave — see `planner_naming_focus`.
        if std::mem::take(&mut view.focus_search) {
            search.request_focus();
        }

        ui.add_space(18.0);
        ui.label(label("SHOW"));
        ui.selectable_value(
            &mut view.filter.outcome,
            OutcomeFilter::Any,
            RichText::new("Everything").size(ARCHIVE_META_SIZE),
        );
        ui.selectable_value(
            &mut view.filter.outcome,
            OutcomeFilter::Finished,
            RichText::new("Finished").size(ARCHIVE_META_SIZE),
        )
        .on_hover_text("Things you ticked off");
        ui.selectable_value(
            &mut view.filter.outcome,
            OutcomeFilter::Dropped,
            RichText::new("Dropped").size(ARCHIVE_META_SIZE),
        )
        .on_hover_text("Things you deleted");

        ui.add_space(18.0);
        ui.selectable_value(
            &mut view.filter.kind,
            KindFilter::Any,
            RichText::new("Both").size(ARCHIVE_META_SIZE),
        );
        ui.selectable_value(
            &mut view.filter.kind,
            KindFilter::Tasks,
            RichText::new("Tasks").size(ARCHIVE_META_SIZE),
        );
        ui.selectable_value(
            &mut view.filter.kind,
            KindFilter::Events,
            RichText::new("Events").size(ARCHIVE_META_SIZE),
        );

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(14.0);
            // Only worth saying when the filter is actually hiding something.
            let count = if shown == total {
                format!("{total} rows")
            } else {
                format!("{shown} of {total}")
            };
            ui.label(label(&count));

            // Said out loud rather than swallowed. Lines this build cannot
            // parse are kept in the file untouched, but a count that is not
            // zero is a fact about the user's own data and they should hear it
            // — the old reader dropped such lines silently and mis-paged the
            // ones around them.
            if unreadable > 0 {
                ui.add_space(10.0);
                ui.label(
                    RichText::new(format!(
                        "{unreadable} line{} unreadable",
                        if unreadable == 1 { "" } else { "s" }
                    ))
                    .font(FontId::new(ARCHIVE_FINE_SIZE, FontFamily::Name("space".into())))
                    .color(Color32::from_rgb(255, 150, 120)),
                )
                .on_hover_text("Kept in the file exactly as they are, and left alone");
            }
        });
    });
    ui.add_space(6.0);
}

/// The ledger: the rows, under the month they fall in, newest first.
fn archive_ledger(
    ui: &mut Ui,
    rows: &[&Archived],
    months: &[archive::MonthRun],
    view: &mut ArchiveView,
    palette: [Color32; 6],
    body_height: f32,
) {
    if rows.is_empty() {
        ui.add_space(28.0);
        ui.vertical_centered(|ui| {
            let (headline, hint) = if view.filter.is_open() {
                (
                    "Nothing here yet.",
                    "Finish something, or delete it, and it is kept here.",
                )
            } else {
                ("Nothing matches.", "Widen the search, or show everything.")
            };
            ui.label(RichText::new(headline).size(ARCHIVE_NAME_SIZE));
            ui.label(
                RichText::new(hint)
                    .size(ARCHIVE_META_SIZE)
                    .color(Color32::from_white_alpha(130)),
            );
        });
        // The body still claims its height, so the footer does not float up the
        // window when a search happens to match nothing.
        ui.allocate_space(vec2(ui.available_width(), (body_height - 80.0).max(0.0)));
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt("archive_ledger")
        .scroll_source(egui::scroll_area::ScrollSource::ALL)
        .max_height(body_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let row_width = (ui.available_width() - 24.0).max(320.0);

            for month in months {
                archive_month_heading(ui, &month.label, month.rows.len(), row_width);
                for row in &rows[month.rows.clone()] {
                    let selected = view.selected == Some(row.key());
                    if archive_row(ui, row, selected, palette, row_width).clicked() {
                        // Clicking the selected row again clears it, so the
                        // footer can be put away without hunting for somewhere
                        // neutral to click.
                        view.selected = (!selected).then(|| row.key());
                    }
                }
                ui.add_space(10.0);
            }
        });
}

fn archive_month_heading(ui: &mut Ui, label: &str, count: usize, width: f32) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.set_width(width);
        ui.label(
            RichText::new(label.to_uppercase())
                .font(FontId::new(ARCHIVE_MONTH_SIZE, FontFamily::Name("space".into())))
                .color(Color32::from_white_alpha(150)),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{count}"))
                    .font(FontId::new(ARCHIVE_MONTH_SIZE, FontFamily::Name("space".into())))
                    .color(Color32::from_white_alpha(90)),
            );
        });
    });
    ui.add_space(4.0);
}

/// One row: the date it left the board down the left, then what it was and how
/// it went.
fn archive_row(
    ui: &mut Ui,
    row: &Archived,
    selected: bool,
    palette: [Color32; 6],
    width: f32,
) -> egui::Response {
    let accent = accent_for(&palette, row.color_id());
    // Finished and merely-gone are told apart by the mark rather than by the
    // row's colour: the palette entry still says what *kind* of thing it was,
    // and overriding it would throw that away for a fact the glyph carries.
    let mark_color = if row.was_finished() {
        accent
    } else {
        Color32::from_white_alpha(110)
    };

    let response = egui::Frame::new()
        .fill(if selected {
            Color32::from_white_alpha(20)
        } else {
            Color32::TRANSPARENT
        })
        .stroke(Stroke::new(if selected { 1.4 } else { 0.0 }, accent))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(width - 16.0);
            ui.horizontal_top(|ui| {
                // The date spine. Monospace, so the column of dates reads as a
                // column rather than as ragged text.
                ui.vertical(|ui| {
                    ui.set_width(ARCHIVE_DATE_COLUMN);
                    ui.label(
                        RichText::new(row.archived_at.format("%-d %b").to_string())
                            .font(FontId::new(ARCHIVE_META_SIZE, FontFamily::Monospace))
                            .color(Color32::from_white_alpha(215)),
                    );
                    ui.add_space(-4.0);
                    ui.label(
                        RichText::new(row.archived_at.format("%a %H:%M").to_string())
                            .font(FontId::new(ARCHIVE_FINE_SIZE, FontFamily::Name("space".into())))
                            .color(Color32::from_white_alpha(115)),
                    );
                });

                ui.label(
                    RichText::new(row.mark())
                        .size(ARCHIVE_NAME_SIZE)
                        .color(mark_color),
                );
                ui.add_space(2.0);

                ui.vertical(|ui| {
                    ui.add(
                        Label::new(
                            RichText::new(&row.name)
                                .size(ARCHIVE_NAME_SIZE)
                                .color(Color32::from_white_alpha(if selected { 245 } else { 210 })),
                        )
                        .wrap()
                        .selectable(false),
                    );
                    ui.add_space(-2.0);
                    // The line the whole redesign exists to be able to write.
                    ui.add(
                        Label::new(
                            RichText::new(row.verdict())
                                .font(FontId::new(
                                    ARCHIVE_FINE_SIZE,
                                    FontFamily::Name("space".into()),
                                ))
                                .color(Color32::from_white_alpha(155)),
                        )
                        .wrap()
                        .selectable(false),
                    );
                });
            });
        })
        .response
        .interact(egui::Sense::click());

    // Painted after the frame rather than baked into its fill, because whether
    // the pointer is over a row is only known once the row has been laid out.
    if response.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            CornerRadius::same(8),
            Stroke::new(1.0, Color32::from_white_alpha(45)),
            StrokeKind::Inside,
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.add_space(2.0);
    response
}

/// What the selected row is, in full, and the two things that can be done to
/// it. Reserved whether or not anything is selected, so selecting does not
/// shift the ledger under the pointer.
fn archive_footer(
    ui: &mut Ui,
    rows: &[&Archived],
    view: &mut ArchiveView,
    action: &mut Option<ArchiveAction>,
) {
    // A selection can survive its row: restoring or forgetting rebuilds the
    // list, and so does typing in the search field.
    let selected = view
        .selected
        .and_then(|key| rows.iter().copied().find(|row| row.key() == key));

    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.set_height(ARCHIVE_FOOTER_HEIGHT - 10.0);

        let Some(row) = selected else {
            ui.label(
                RichText::new("Pick a row to put it back, or to forget it for good.")
                    .size(ARCHIVE_META_SIZE)
                    .color(Color32::from_white_alpha(130)),
            );
            return;
        };

        ui.vertical(|ui| {
            ui.add(
                Label::new(RichText::new(&row.name).size(ARCHIVE_NAME_SIZE).strong())
                    .truncate()
                    .selectable(false),
            );
            ui.add_space(-2.0);
            // The timestamps in full — the one thing the row above deliberately
            // does not spell out, because a column of them would be unreadable.
            let mut facts = vec![format!(
                "written {}",
                row.created.format("%-d %b %Y %H:%M")
            )];
            if let Some(deadline) = row.deadline {
                facts.push(format!(
                    "{} {}",
                    if row.is_event { "was set for" } else { "due" },
                    deadline.format("%-d %b %Y %H:%M")
                ));
            }
            facts.push(format!(
                "{} {}",
                if row.was_finished() { "finished" } else { "removed" },
                row.archived_at.format("%-d %b %Y %H:%M")
            ));
            ui.label(
                RichText::new(facts.join("  ·  "))
                    .font(FontId::new(ARCHIVE_FINE_SIZE, FontFamily::Name("space".into())))
                    .color(Color32::from_white_alpha(160)),
            );
        });

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(14.0);
            if ui
                .button(RichText::new("Forget").size(ARCHIVE_META_SIZE))
                .on_hover_text("Remove it from the archive for good")
                .clicked()
            {
                view.confirm_forget = Some(row.key());
            }
            // Restoring asks nothing: it is undone by ticking the thing off
            // again, which is a button away. Forgetting is the one act in the
            // app with no way back, so it is the one that asks.
            if ui
                .button(RichText::new("↩ Put it back").size(ARCHIVE_META_SIZE))
                .on_hover_text("Return it to the board, with its plan and its dates")
                .clicked()
            {
                *action = Some(ArchiveAction::Restore(row.key()));
            }
        });
    });
}

fn archive_forget_confirmation(
    ctx: &Context,
    rows: &[&Archived],
    view: &mut ArchiveView,
    action: &mut Option<ArchiveAction>,
) {
    let Some(key) = view.confirm_forget else { return };
    // The row can vanish underneath the dialog (a search narrowing, a restore);
    // dismiss rather than act on something that is no longer there.
    let Some(row) = rows.iter().copied().find(|row| row.key() == key) else {
        view.confirm_forget = None;
        return;
    };

    egui::Window::new("Forget this?")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(format!("Forget \"{}\"?", row.name));
            ui.label(
                RichText::new("This one is not kept. It leaves the archive for good.")
                    .size(ARCHIVE_META_SIZE)
                    .color(Color32::from_white_alpha(160)),
            );
            let (accepted, dismissed) = confirmation_keys(ui.ctx());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(Button::new("Yes").min_size(CONFIRM_BUTTON)).clicked() || accepted {
                    *action = Some(ArchiveAction::Forget(key));
                }
                if ui.add(Button::new("No").min_size(CONFIRM_BUTTON)).clicked() || dismissed {
                    view.confirm_forget = None;
                }
                confirmation_key_hint(ui);
            });
        });
}

/// "14:30–15:15" for an event's footer, from its time and length; just the
/// time when it has no length yet.
fn item_anchor_text(deadline: Option<DateTime<Local>>, minutes: Option<u32>) -> Option<String> {
    let at = deadline?;
    Some(match minutes {
        Some(minutes) => format!(
            "{}–{}",
            at.format("%H:%M"),
            (at + Duration::minutes(minutes as i64)).format("%H:%M")
        ),
        None => at.format("%H:%M").to_string(),
    })
}

/// Whether a yes/no dialog was answered from the keyboard this frame, as
/// `(accepted, dismissed)`.
///
/// A confirmation is a question with an obvious answer key: reaching for the
/// mouse to click "Yes" on a dialog you raised with `Del` a moment ago is the
/// kind of thing that makes a keyboard-driven view stop being one. Enter agrees,
/// Escape backs out.
///
/// Dismissal wins if somehow both arrive in one frame — backing out of a
/// destructive action is the safe reading.
fn confirmation_keys(ctx: &Context) -> (bool, bool) {
    let (accepted, dismissed) = ctx.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
    (accepted && !dismissed, dismissed)
}

/// Size for a yes/no button, so the pair reads as a choice rather than as two
/// words jammed against the bottom of the window.
const CONFIRM_BUTTON: Vec2 = Vec2::new(76.0, 32.0);

/// The line that tells you the keys exist. Without it `confirmation_keys` is a
/// shortcut nobody discovers, and the mouse trip it saves is the whole point.
fn confirmation_key_hint(ui: &mut Ui) {
    ui.add_space(10.0);
    ui.label(
        RichText::new("Enter · Esc")
            .size(13.0)
            .color(Color32::from_white_alpha(110)),
    );
}

struct FpsCounter {
    last_update: Instant,
    frame_count: u32,
    fps_text: String,
}

struct PressState {
    idx: usize,
    press_pos: Pos2,
    cancelled: bool,
}

/// The colour a cell draws one preview item in.
///
/// A subscribed calendar's events carry their own colour rather than a palette
/// index: it says *which calendar*, not how urgent, so it deliberately does not
/// follow the colour scheme (§23).
fn preview_color(palette: &[Color32; 6], item: &PreviewItem) -> Color32 {
    match item.subscribed {
        Some([r, g, b, _]) => Color32::from_rgb(r, g, b),
        None => palette[item.color_id.min(5)],
    }
}

/// One item in a day cell's compact preview (at most 3 are shown in the cell).
#[derive(Clone)]
struct PreviewItem {
    name: String,
    /// "HH:MM", or empty for an undated item.
    time: String,
    /// Palette index (see `Active::calendar_item_color`).
    color_id: usize,
    /// Set when this came off a subscribed calendar (§23), carrying that
    /// calendar's own colour rather than a palette index — the colour says
    /// *which calendar*, not how urgent. `None` for the board's own things.
    subscribed: Option<[u8; 4]>,
}

/// One day cell of the calendar model, cached in `TaskApp::calendar_elements`
/// and consumed by `show_calendar`. Named fields replace what used to be an
/// opaque positional 6-tuple.
#[derive(Clone)]
struct DayCell {
    preview: Vec<PreviewItem>,
    /// How many items land on the day in total. The cell's layout is chosen by
    /// this (0/1/2/3/4+), and the 4+ case shows an overflow marker. This used to
    /// be a full second copy of the day's items, built for the day popup; the
    /// planner reads `board.items` directly, so only the count is still owed.
    item_count: usize,
    is_today: bool,
    date: NaiveDate,
    label: String,
}

impl FpsCounter {
    fn new() -> Self {
        Self {
            last_update: Instant::now(),
            frame_count: 0,
            fps_text: String::from("FPS: ..."),
        }
    }

    fn update(&mut self) {
        self.frame_count += 1;
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update);
        if elapsed >= std::time::Duration::from_secs(1) {
            let fps = self.frame_count as f32 / elapsed.as_secs_f32();
            self.fps_text = format!("FPS: {:.1}", fps);
            self.frame_count = 0;
            self.last_update = now;
        }
    }
}

pub struct TaskAppConfig {
    pub colorschemes: HashMap<u32, ColorScheme>,
    pub selected_colorscheme_id: u32,
    /// The live set, the archive and the notepad, already read.
    pub board: Board,
    pub dirs: AppDirs,
    pub background: String,
    pub background_options: Vec<String>,
    pub coordinates: [f32; 2],
    pub start_in_fullscreen: bool,
    pub enable_fps_counter: bool,
    pub calendar_weeks_to_show: usize,
    pub selected_monitor_name: String,
    pub three_day_weather: bool,
    pub background_image_tint_percent: u32,
    pub ui_scale_percent: u32,
    pub weather_service: WeatherService,
    /// The subscribed-calendar service, or `None` when this copy is a client:
    /// the board is somebody else's and so is the fetching (§23). An `Option`
    /// rather than a flag, so the rule lives in the type and not in a branch
    /// that has to remember it.
    pub calendars: Option<subscriptions::Feeds>,
    /// Message describing any non-fatal startup recovery (e.g. a corrupt data
    /// file that was quarantined), to surface in the error window once the UI is
    /// up. `None` when startup loaded cleanly.
    pub startup_error: Option<String>,
    /// The phone view (`phone.rs`): whether to serve it, on which port, and
    /// the key its link carries.
    pub phone_enabled: bool,
    pub phone_port: u16,
    /// The address it listens on — `phone::DEFAULT_BIND` or one address.
    pub phone_bind: String,
    pub phone_token: String,
    /// Its request queue — the server thread sends, the UI thread serves.
    pub phone_tx: Sender<phone::PhoneRequest>,
    pub phone_rx: Receiver<phone::PhoneRequest>,
    /// Wakes the event loop from another thread.
    pub event_proxy: EventLoopProxy<()>,
    /// Frame cap while awake, or `FRAME_CAP_UNCAPPED`.
    pub frame_cap_fps: u32,
    /// Running against a `taskdeck-server`: the engine that keeps the replica
    /// in step (`sync.rs`), and the setting it came from.
    pub sync: Option<sync::SyncHandle>,
    pub server_url: String,
    pub server_token: String,
}

pub struct TaskApp {
    /* ───────────────────────── Animation ───────────────────────── */
    row_anim: Vec<f32>,
    last_anim_time: f64,

    smoothed_scroll_velocity: f32,
    scroll_velocity_smoothing_tau: f32,
    animation_intensity: f32,
    smoothed_animation_intensity: f32,
    last_raw_scroll: f32,

    /* ───────────────────────── UI / Context ───────────────────────── */
    background_image_texture: Option<TextureHandle>,
    pending_initial_background: Option<String>,
    dirs: AppDirs,

    hovered_calendar_cell: Option<usize>,
    press_origin: Option<PressState>,

    userconfig_path: PathBuf,

    /* ───────────────────────── Time & Date ───────────────────────── */
    date: DateTime<Local>,
    next_three_weekdays: (String, String, String),
    /// Timestamp of the most recent notepad edit, used to debounce autosave.
    /// `None` once there is nothing pending to save.
    last_textbox_edit_time: Option<Instant>,

    /* ───────────────────────── Tasks & Events ───────────────────────── */
    /// The data: live items, archive, notepad, and every way they change
    /// (`board.rs`). The UI reads it every frame and changes it only through
    /// `apply`.
    board: Board,
    list_tasks: Vec<Active>,
    archive_view: ArchiveView,

    calendar_elements: Vec<DayCell>,

    /* ────────────────── Subscribed calendars (§23) ────────────────── */
    /// `None` on a client: the board is the server's and so is the fetching.
    calendars: Option<subscriptions::Feeds>,
    last_calendars_version: u64,
    /// What the CALENDARS settings section is typing.
    calendar_url_input: String,
    calendar_name_input: String,
    calendar_add_error: Option<String>,

    /* ───────────────────────── Weather ───────────────────────── */
    pub weather_service: WeatherService,
    weather_data_cache: Vec<Vec<(String, f64, i32, bool)>>,
    last_weather_version: u64,
    three_day_weather: bool,
    weather_is_broken_flag: bool,

    /* ───────────────────────── Inputs ───────────────────────── */
    /// Weeks the settings sheet is offering, which is not yet the number the
    /// calendar is drawing: applying rebuilds the whole calendar model, so the
    /// two only meet when the drag stops. See `set_calendar_weeks`.
    week_input: usize,
    task_name_input: String,
    task_importance_input: u8,
    time_importance_input: u8,
    event_name_input: String,

    year_input: i32,
    month_input: i32,
    day_input: i32,
    hour_input: i32,
    minute_input: i32,

    /* ───────────────────────── Flags ───────────────────────── */
    new_task_flag: bool,
    new_event_flag: bool,
    error_flag: bool,
    settings_flag: bool,
    should_save_textbox_text: bool,

    user_wants_to_complete_task_flag: bool,
    user_wants_to_delete_task_flag: bool,
    /// Set for the one frame after a create dialog opens, to put the caret in
    /// its name field. A shortcut that opens a dialog you then have to reach
    /// for the mouse to type into is half a shortcut.
    dialog_wants_focus: bool,

    /* ───────────────────────── Day planner ───────────────────────── */
    /// Whether the planner window is open, and the day it is showing.
    ///
    /// The day is a date rather than a calendar cell index, so the planner can
    /// step forward past the end of the calendar's range — and so a calendar
    /// rebuild underneath it (a midnight rollover, a reduced week count) can't
    /// leave it pointing at the wrong day.
    planner_flag: bool,
    planner_day: NaiveDate,
    /// Item the user last clicked on the timeline or in the tray; the footer
    /// edits it.
    planner_selection: Option<u64>,
    /// Which of the selection's sessions was clicked, when a work block was —
    /// per-block controls (remove, and the gestures) act on this one. `None`
    /// when the selection came from the tray, a marker, or an event block.
    planner_selected_session: Option<usize>,
    /// Task whose deadline is being edited in the footer's due popup, if any.
    planner_due_edit: Option<u64>,
    /// The pointer gesture in progress, if any. One field, because the gestures
    /// are mutually exclusive — you can't resize one block while moving another.
    planner_drag: Option<PlannerDrag>,
    /// Item whose title is being typed. Set right after a create so the block
    /// can be named without leaving the timeline.
    planner_naming: Option<u64>,
    planner_name_input: String,
    /// Whether the item under the title editor was created by the gesture that
    /// opened it. Escape removes such an item outright; on a rename it only
    /// restores the old name. See `discard_planner_naming`.
    planner_naming_created: bool,
    /// Set for the one frame the title editor should claim keyboard focus.
    ///
    /// It must be *one* frame, not every frame. `Response::lost_focus` is a
    /// live query into egui's focus memory, so re-requesting focus each frame
    /// put it back the instant Enter made the field surrender it — the query
    /// then answered "no", and Enter could never finish the edit at all.
    planner_naming_focus: bool,
    /// What a drag (or double-click) on empty timeline creates.
    planner_create_kind: planner::CreateKind,
    /// The tray's quick-add field: a name typed here becomes an undated,
    /// unplanned task, ready to be dragged onto the timeline. Getting something
    /// out of your head and onto the day should not need a modal.
    planner_quick_add_input: String,
    /// Set for one frame after the planner opens or changes day, to scroll the
    /// timeline to a useful hour rather than to midnight.
    planner_scroll_to_hour: Option<f32>,
    /// What the shown day was actually spent on: the blocks of items that have
    /// since been finished or dropped, drawn behind what is still planned.
    ///
    /// Cached rather than derived each frame. Building it means walking the
    /// whole archive, and the planner redraws continuously — but the answer
    /// only changes when the day does or when something is archived, restored
    /// or forgotten. See `rebuild_planner_ghosts`.
    planner_ghosts: Vec<PlannerGhost>,

    /* ───────────────────────── Settings ───────────────────────── */
    start_in_fullscreen: bool,
    enable_fps_counter: bool,
    calendar_weeks_to_show: usize,

    selected_background_index: usize,
    background_options: Vec<String>,
    background_image_tint_percent: u32,
    /// Percentage the whole UI is scaled by, or `UI_SCALE_AUTO` for the
    /// fit-to-window default. See `apply_ui_scale`.
    ui_scale_percent: u32,
    /// The explicit percentage the settings slider edits, kept while the scale
    /// is set to fit automatically — otherwise turning the automatic fit off
    /// would have no number to turn it off *to*.
    ui_scale_input: u32,
    /// Points-per-pixel the text styles were last snapped for. See
    /// `apply_ui_scale` and `snap_font_points`.
    last_font_ppp: f32,
    /// Frames per second the loop is held to while awake, or
    /// `FRAME_CAP_UNCAPPED`. Read by `App::schedule_next_frame` every frame,
    /// so a change in Settings applies at once.
    frame_cap_fps: u32,
    /// The rate the settings slider edits, kept while the cap is off so
    /// switching it on has a number to switch on *to*.
    frame_cap_input: u32,

    /* ───────────────────────── Errors & Confirmations ───────────────────────── */
    /// Id of the item awaiting a complete/delete confirmation. The dialog looks
    /// up the (cosmetic) name from this id for display.
    confirm_complete_task: Option<u64>,
    confirm_delete_task: Option<u64>,
    error_text: String,

    /* ───────────────────────── FPS / Monitor ───────────────────────── */
    fps_counter: FpsCounter,
    selected_monitor_name: String,
    pub monitor_options: Vec<String>, //this needs to be set as public because it is edited by the owner of the taskapp struct at runtime
    selected_monitor_index: usize,

    /* ───────────────────────── Map ───────────────────────── */
    coordinates_map_flag: bool,
    map_zoom: f32,
    map_offset: Vec2,
    /// Where the weather was pointed when the map picker opened, so Cancel has
    /// something to go back to. The map edits `coordinates` live — that is what
    /// makes the crosshair follow the click — so without this there is no way
    /// out of the window that isn't a change.
    coordinates_before_picker: Option<[f32; 2]>,
    /// Live, editable coordinates `[lat, lon]` — the single source of truth for
    /// the map picker. Pushed to the weather service (its own thread-local copy)
    /// and persisted only on confirm, via `set_weather_coordinates`.
    coordinates: [f32; 2],
    map_texture: Option<TextureHandle>,

    /* ───────────────────────── Color Schemes ───────────────────────── */
    color_picker_flag: bool,
    colorschemes: HashMap<u32, ColorScheme>,
    active_colorscheme: [Color32; 6],
    selected_colorscheme_id: u32,

    rename_colorscheme_flag: bool,
    colorscheme_rename_input: String,
    user_wants_to_delete_colorscheme_flag: bool,
    edit_colorscheme_flag: bool,
    colorscheme_being_edited: Option<ColorScheme>,
    dragged_color_index: Option<usize>,

    /* ───────────────────────── Calendar ───────────────────────── */
    row_contains_month_switch: Vec<Option<(String, String)>>,

    /* ───────────────────────── Phone view ───────────────────────── */
    phone_enabled: bool,
    phone_port: u16,
    /// Where it listens; from the file only (§21.7).
    phone_bind: String,
    /// The port the settings sheet is offering; applied when it settles.
    phone_port_input: u16,
    phone_token: String,
    /// The listening server while the view is on. `None` while it is off —
    /// and briefly after a restart, while the old socket closes.
    phone_server: Option<phone::PhoneServer>,
    /// Why the server is not running when it should be.
    phone_error: Option<String>,
    /// While set, `tend_phone_server` keeps trying to bind until this instant
    /// before giving up and reporting the failure.
    phone_retry_until: Option<Instant>,
    phone_last_attempt: Option<Instant>,
    phone_tx: Sender<phone::PhoneRequest>,
    phone_rx: Receiver<phone::PhoneRequest>,
    event_proxy: EventLoopProxy<()>,
    /// The board's version, published to the server's threads so a phone
    /// waiting on `/api/wait` is answered the moment it moves (§21.2).
    phone_pulse: Arc<phone::Pulse>,
    /// The addresses Settings points the phone at, and when they were looked up.
    phone_addresses: Vec<String>,
    phone_addresses_checked: Option<Instant>,
    /// The QR code last painted, keyed by the link it encodes.
    phone_qr: Option<(String, usize, Vec<bool>)>,

    /* ───────────────────────── Server ───────────────────────── */
    /// The sync engine, when this copy is a client of a `taskdeck-server`.
    sync: Option<sync::SyncHandle>,
    /// The settings sheet's fields. Applied at the next start: the board is
    /// fetched, and the engine started, before there is a window.
    server_url_input: String,
    server_token_input: String,

    /* ───────────────────────── Misc ───────────────────────── */
    use_date_for_addable: bool,
}

impl TaskApp {
    pub fn new(config: TaskAppConfig) -> Self {
        let now = Local::now();

        let active_colorscheme =
            resolve_colorscheme(&config.colorschemes, config.selected_colorscheme_id);

        let selected_background_index = config
            .background_options
            .iter()
            .position(|b| b == &config.background)
            .unwrap_or(0);

        let board = config.board;

        let userconfig_path = config.dirs.config_file();

        let mut app = Self {
            /* Animation */
            row_anim: Vec::new(),
            last_anim_time: 0.0,
            smoothed_scroll_velocity: 0.0,
            scroll_velocity_smoothing_tau: 0.12,
            animation_intensity: 1.0,
            smoothed_animation_intensity: 0.0,
            last_raw_scroll: 0.0,

            /* UI */
            background_image_texture: None,
            pending_initial_background: Some(config.background),
            dirs: config.dirs,
            hovered_calendar_cell: None,
            press_origin: None,
            userconfig_path,

            /* Time */
            date: now,
            next_three_weekdays: next_three_weekdays(now),
            last_textbox_edit_time: None,

            /* Tasks */
            list_tasks: board.items.iter().filter(|t| !t.is_event).cloned().collect(),
            board,
            archive_view: ArchiveView::default(),
            calendar_elements: Vec::new(),

            /* Weather */
            weather_service: config.weather_service,
            weather_data_cache: Vec::new(),
            last_weather_version: 0,
            calendars: config.calendars,
            last_calendars_version: 0,
            calendar_url_input: String::new(),
            calendar_name_input: String::new(),
            calendar_add_error: None,
            three_day_weather: config.three_day_weather,
            weather_is_broken_flag: false,

            /* Inputs */
            week_input: config.calendar_weeks_to_show,
            task_name_input: String::new(),
            task_importance_input: 2,
            time_importance_input: 1,
            event_name_input: String::new(),

            year_input: now.year(),
            month_input: now.month() as i32,
            day_input: now.day() as i32,
            hour_input: now.hour() as i32,
            minute_input: now.minute() as i32,

            /* Flags */
            new_task_flag: false,
            new_event_flag: false,
            error_flag: config.startup_error.is_some(),
            settings_flag: false,
            user_wants_to_complete_task_flag: false,
            user_wants_to_delete_task_flag: false,
            dialog_wants_focus: false,

            /* Day planner */
            planner_flag: false,
            planner_day: now.date_naive(),
            planner_selection: None,
            planner_selected_session: None,
            planner_due_edit: None,
            planner_drag: None,
            planner_naming: None,
            planner_name_input: String::new(),
            planner_naming_created: false,
            planner_naming_focus: false,
            planner_create_kind: planner::CreateKind::default(),
            planner_quick_add_input: String::new(),
            planner_scroll_to_hour: None,
            planner_ghosts: Vec::new(),
            should_save_textbox_text: false,

            /* Settings */
            start_in_fullscreen: config.start_in_fullscreen,
            enable_fps_counter: config.enable_fps_counter,
            calendar_weeks_to_show: config.calendar_weeks_to_show,

            selected_background_index,
            background_options: config.background_options,
            background_image_tint_percent: config.background_image_tint_percent,
            ui_scale_percent: config.ui_scale_percent,
            // Fitting automatically leaves no percentage to show, so the slider
            // starts from the top of the range rather than from zero.
            ui_scale_input: if config.ui_scale_percent == UI_SCALE_AUTO {
                UI_SCALE_MAX
            } else {
                config.ui_scale_percent
            },
            last_font_ppp: 0.0,
            frame_cap_fps: config.frame_cap_fps,
            frame_cap_input: if config.frame_cap_fps == FRAME_CAP_UNCAPPED {
                FRAME_CAP_DEFAULT
            } else {
                config.frame_cap_fps
            },

            /* Errors */
            confirm_complete_task: None,
            confirm_delete_task: None,
            error_text: config.startup_error.unwrap_or_default(),

            /* FPS / Monitor */
            fps_counter: FpsCounter::new(),
            selected_monitor_name: config.selected_monitor_name,
            monitor_options: Vec::new(),
            selected_monitor_index: 0,

            /* Map */
            coordinates_map_flag: false,
            map_zoom: 1.0,
            map_offset: Vec2::ZERO,
            coordinates: config.coordinates,
            coordinates_before_picker: None,
            map_texture: None,

            /* Colors */
            color_picker_flag: false,
            colorschemes: config.colorschemes,
            active_colorscheme,
            selected_colorscheme_id: config.selected_colorscheme_id,

            rename_colorscheme_flag: false,
            colorscheme_rename_input: String::new(),
            user_wants_to_delete_colorscheme_flag: false,
            edit_colorscheme_flag: false,
            colorscheme_being_edited: None,
            dragged_color_index: None,

            /* Calendar */
            row_contains_month_switch: Vec::new(),

            /* Phone view */
            phone_enabled: config.phone_enabled,
            phone_port: config.phone_port,
            phone_bind: config.phone_bind,
            phone_port_input: config.phone_port,
            phone_token: config.phone_token,
            phone_server: None,
            phone_error: None,
            phone_retry_until: None,
            phone_last_attempt: None,
            phone_tx: config.phone_tx,
            phone_rx: config.phone_rx,
            event_proxy: config.event_proxy,
            phone_pulse: Arc::new(phone::Pulse::new()),
            phone_addresses: Vec::new(),
            phone_addresses_checked: None,
            phone_qr: None,

            /* Server */
            sync: config.sync,
            server_url_input: config.server_url,
            server_token_input: config.server_token,

            /* Misc */
            use_date_for_addable: true,
        };

        // Listening from the first moment rather than the first frame: a phone
        // asking while the window is still coming up gets an answer as soon
        // as the event loop is running.
        app.tend_phone_server();
        app
    }

    fn sync_calendar_caches(&mut self) {
        if self.row_anim.len() != self.calendar_weeks_to_show {
            self.row_anim.resize(self.calendar_weeks_to_show, 0.0);
        }
    }

    pub fn init_with_context(&mut self, ctx: &Context) {
        load_fonts(ctx);
        set_styles(ctx);

        if self.start_in_fullscreen {
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
        }

        self.sync_calendar_caches();

        self.fix_and_cache_weather_data();

        egui_extras::install_image_loaders(ctx);

        self.map_texture = Some(set_world_map(ctx));
    }

    fn refilter_tasks(&mut self) {
        // Neither events nor routines belong in the ranked list: an event is
        // not work you owe, and a routine is a standing arrangement that would
        // sit there forever with no ✓ that could ever clear it.
        self.list_tasks = self
            .board.items
            .iter()
            .filter(|task| !task.is_event && !task.is_routine())
            .cloned()
            .collect();
    }

    fn show_tasks(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
        // egui 0.34+ changed the ScrollArea drag default to `DragScroll::OnTouch` (mouse drag no
        // longer scrolls). `ScrollSource::ALL` restores the 0.33 default (scrollbar + wheel +
        // mouse-drag) so click-drag scrolling works as before.
        .scroll_source(egui::scroll_area::ScrollSource::ALL)
        .wheel_scroll_multiplier(vec2(1.0, 1.5))
        .show(ui, |ui| {
            ui.set_min_size(egui::Vec2 { x: 290.0, y: 50.0 });
            ui.set_width(300f32);
            ui.vertical(|ui| {
                for task in self.list_tasks.iter() {
                    egui::Frame::new()
                        .fill(Color32::from_black_alpha(60))
                        .stroke(egui::Stroke::new(1.5, Color32::from_white_alpha(55)))
                        .corner_radius(egui::CornerRadius::same(14))
                        .inner_margin(Margin::symmetric(12, 12))
                        .show(ui, |ui| {
                            ui.set_width(245.0);
                            ui.set_min_size(egui::Vec2 { x: 258.0, y: 40.0 });
                            ui.set_max_size(egui::Vec2 { x: 245.0, y: 40.0 });
                            ui.horizontal(|ui| {
                                let task_font = FontId::new(17.0, FontFamily::Name("bungee".into()));
                                ui.set_width(245.0);
                                ui.set_min_size(egui::Vec2 { x: 245.0, y: 40.0 });
                                ui.set_max_size(egui::Vec2 { x: 245.0, y: 40.0 });
                                ui.add(Label::new(RichText::new(&task.name).color(Color32::from_white_alpha(120)).font(task_font)).wrap().selectable(false));
                                
                                if ui.ui_contains_pointer() {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let min_button_size = Vec2::new(28.0, 28.0);

                                        let complete_button = egui::Button::new("✓").min_size(min_button_size).corner_radius(CornerRadius::same(8));
                                        let delete_button = egui::Button::new("x").min_size(min_button_size).corner_radius(CornerRadius::same(8));
                                        if ui.add(complete_button).clicked() {
                                            self.user_wants_to_complete_task_flag = true;
                                            self.confirm_complete_task = Some(task.id);
                                        }

                                        if ui.add(delete_button).clicked() {
                                            self.user_wants_to_delete_task_flag = true;
                                            self.confirm_delete_task = Some(task.id);
                                        }
                                    });
                                };
                            });
                        });
                }
            });
        });
    }

    /// One day's forecast: twelve two-hour slots in a 4×3 grid.
    ///
    /// Every cell is **allocated at exactly `WEATHER_CELL`** and painted, rather
    /// than laid out from its contents. Laid out, the cell was as wide as its
    /// widest line — so a slot reading `-34` was twenty points wider than one
    /// reading `7`, the column grew to fit it, and the whole grid changed shape
    /// when the forecast did. A wall calendar that reflows because it got cold
    /// is not a wall calendar.
    fn display_stuff(&self, thing: &Vec<(String, f64, i32, bool)>, ui: &mut Ui, grid_id: String, upper_day: bool) {
        egui::Grid::new(grid_id)
            .spacing(Vec2::new(WEATHER_CELL_GAP, WEATHER_CELL_GAP))
            .min_col_width(WEATHER_CELL.x)
            .max_col_width(WEATHER_CELL.x)
            .show(ui, |ui| {
                let nth_cell_to_highlight = match self.date.hour() {
                    0..2 => 0,
                    2..4 => 1,
                    4..6 => 2,
                    6..8 => 3,
                    8..10 => 4,
                    10..12 => 5,
                    12..14 => 6,
                    14..16 => 7,
                    16..18 => 8,
                    18..20 => 9,
                    20..22 => 10,
                    _ => 11,
                };
                for (i, (time, temp, wmo_code, is_day)) in thing.iter().enumerate() {
                    let now = i == nth_cell_to_highlight && upper_day;
                    let (rect, _) = ui.allocate_exact_size(WEATHER_CELL, egui::Sense::hover());

                    ui.painter().rect_stroke(
                        rect,
                        CornerRadius::same(15),
                        if now {
                            Stroke::new(0.6, Color32::WHITE)
                        } else {
                            Stroke::new(0.5, Color32::from_white_alpha(150))
                        },
                        StrokeKind::Inside,
                    );

                    let ink = if now { Color32::WHITE } else { Color32::from_white_alpha(120) };

                    // The hour, along the top.
                    ui.painter().text(
                        pos2(rect.left() + 10.0, rect.top() + 7.0),
                        egui::Align2::LEFT_TOP,
                        time,
                        FontId::new(13.5, FontFamily::Name("space".into())),
                        ink,
                    );

                    // The sky, filling the bottom of the cell: centred, and
                    // sitting on the floor of it. The readings are laid over its
                    // top corners, which have very little sky in them — that
                    // overlap is the arrangement, not an accident of it.
                    egui::Image::new(weather::icon_for_wmo(*wmo_code, *is_day).clone()).paint_at(
                        ui,
                        Rect::from_center_size(
                            pos2(rect.center().x, rect.bottom() - WEATHER_ICON * 0.5 - 2.0),
                            Vec2::splat(WEATHER_ICON),
                        ),
                    );

                    // The temperature under the hour and against the right edge:
                    // it is the one thing in the cell whose width isn't known in
                    // advance, so it grows towards a fixed edge rather than
                    // pushing one.
                    ui.painter().text(
                        pos2(rect.right() - 9.0, rect.top() + 20.0),
                        egui::Align2::RIGHT_TOP,
                        format!("{temp:.0}"),
                        FontId::new(18.0, FontFamily::Monospace),
                        ink,
                    );

                    if (i + 1) % 4 == 0 {
                        ui.end_row();
                    }
                }
            });
    }

    fn show_weather_forecast(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            if self.weather_is_broken_flag {
                // No usable forecast yet — either the first fetch hasn't completed
                // or the data arrived in an unexpected shape. Show a small notice in
                // place of the grids; the notepad below stays reachable regardless.
                ui.add_space(WEATHER_DAY_GAP);
                weather_day_label(ui, "WEATHER IS BROKEN");
            } else {
                weather_day_label(ui, &self.next_three_weekdays.0.clone());
                ui.add_space(WEATHER_LABEL_GAP);

                let day_1 = &self.weather_data_cache[0];
                self.display_stuff(day_1, ui, "firstweathergrid".to_string(), true);

                ui.add_space(WEATHER_DAY_GAP);
                weather_day_label(ui, &self.next_three_weekdays.1.clone());
                ui.add_space(WEATHER_LABEL_GAP);

                let day_2 = &self.weather_data_cache[1];
                self.display_stuff(day_2, ui, "secondweathergrid".to_string(), false);

                if self.three_day_weather {
                    ui.add_space(WEATHER_DAY_GAP);
                    weather_day_label(ui, &self.next_three_weekdays.2.clone());
                    ui.add_space(WEATHER_LABEL_GAP);

                    let day_3 = &self.weather_data_cache[2];
                    self.display_stuff(day_3, ui, "thirdweathergrid".to_string(), false);
                }
            }

            // The notepad occupies the right panel whenever 3-day weather is off.
            // It is deliberately decoupled from `weather_is_broken_flag` so a failed
            // or still-pending weather fetch can never hide the user's notes
            // (CODE_REVIEW A6).
            if !self.three_day_weather {
                self.show_notepad(ui);
            }
        });
    }

    /// The notepad: the bottom of the right column whenever the third day of
    /// weather is switched off.
    ///
    /// It is drawn as one more cell of the weather column — the same hairline
    /// stroke, the same corner, no fill of its own — and it is exactly as wide
    /// as the grid above it. The first version of this card was a heavy dark
    /// slab, wider than the forecast it sat under, with generous padding eating
    /// the space the notes were supposed to have: three separate ways of
    /// announcing itself in a column whose whole manner is to be quiet.
    fn show_notepad(&mut self, ui: &mut Ui) {
        ui.add_space(12.0);

        // What the column has left under the forecast, whatever the UI scale
        // has made of it — measured here, in the column's own vertical ui,
        // because that is the only place the answer is meaningful.
        let card_height = (ui.available_height() - NOTEPAD_BOTTOM_MARGIN)
            .clamp(NOTEPAD_MIN_HEIGHT, NOTEPAD_MAX_HEIGHT);

        ui.horizontal(|ui| {
            egui::Frame::new()
                .stroke(Stroke::new(0.5, Color32::from_white_alpha(150)))
                .corner_radius(CornerRadius::same(15))
                .inner_margin(Margin::same(NOTEPAD_PADDING as i8))
                .show(ui, |ui| {
                    ui.set_width(NOTEPAD_WIDTH);

                    // A `Frame` inherits the layout it is placed in, and this
                    // one is placed in a row — so without this the heading and
                    // the writing area were laid out *side by side*, and the
                    // note wrapped at two characters in what was left over.
                    ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("NOTES")
                                .font(FontId::new(NOTEPAD_HEADING_SIZE, FontFamily::Name("space".into())))
                                .color(Color32::from_white_alpha(120)),
                        );
                        // Quiet reassurance rather than a status bar: the
                        // autosave is on a two-second debounce, so there is a
                        // moment where a keystroke is only in memory, and this
                        // is that moment made visible.
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if self.should_save_textbox_text {
                                ui.label(
                                    RichText::new("unsaved")
                                        .font(FontId::new(NOTEPAD_HEADING_SIZE, FontFamily::Name("space".into())))
                                        .color(Color32::from_white_alpha(85)),
                                );
                            }
                        });
                    });
                    ui.add_space(8.0);

                    // The writing area is given the height, not the card: a
                    // `set_height` on the card's own ui makes the heading row
                    // inherit it and centre itself down the middle of the note.
                    //
                    // Both bounds of the scroll area, so it is exactly this tall
                    // whatever is in it — `max_height` alone lets it shrink to
                    // its content and the card closes up like a fan.
                    //
                    // The field then asks for as many rows as that fits, from
                    // the real row height of the face it is set in rather than a
                    // guessed line spacing, so the caret starts at the top of
                    // the card and an empty note is the same rectangle as a full
                    // one.
                    let text_height = card_height - NOTEPAD_CHROME_HEIGHT;
                    let font = FontId::new(NOTEPAD_TEXT_SIZE, FontFamily::Monospace);
                    let row_height = ui
                        .ctx()
                        .fonts_mut(|fonts| fonts.row_height(&font))
                        .max(1.0);
                    let rows = ((text_height / row_height).floor() as usize).max(4);

                    egui::ScrollArea::vertical()
                        .id_salt("notepad")
                        .auto_shrink([false, false])
                        .min_scrolled_height(text_height)
                        .max_height(text_height)
                        .max_width(NOTEPAD_WIDTH)
                        .show(ui, |ui| {
                            let response = ui.add(
                                egui::TextEdit::multiline(&mut self.board.notes)
                                    // The card is the frame; a second one drawn
                                    // inside it is just a box in a box.
                                    .frame(egui::Frame::NONE)
                                    .margin(Margin::ZERO)
                                    .hint_text("anything worth keeping in front of you")
                                    // The card's width, less room for the
                                    // scrollbar to sit beside the text rather
                                    // than over the ends of the lines.
                                    .desired_width(NOTEPAD_WIDTH - 10.0)
                                    .desired_rows(rows)
                                    .font(font.clone()),
                            );

                            if response.changed() {
                                // Typed or pasted tabs would be painted as
                                // missing-glyph boxes (see `utilities::detab`).
                                // Nothing inserts one any more — the field is no
                                // longer a code editor, so Tab leaves it — but
                                // a paste still can.
                                if self.board.notes.contains('\t') {
                                    self.board.notes = utilities::detab(&self.board.notes);
                                }
                                self.should_save_textbox_text = true;
                                self.last_textbox_edit_time = Some(Instant::now());
                            }
                        });
                    });
                });
        });
    }

    fn show_calendar(&mut self, ui: &mut egui::Ui) {
        #[inline]
        fn ease_out_quintic(x: f32) -> f32 {
            let t = x.clamp(0.0, 1.0);
            1.0 - (1.0 - t).powi(5)
        }

        #[inline]
        fn ease_out_square(x: f32) -> f32 {
            let t = x.clamp(0.0, 1.0);
            1.0 - (1.0 - t).powi(2)
        }

        #[inline]
        fn exponential_smooth(prev: f32, sample: f32, tau: f32, dt: f32) -> f32 {
            if dt <= 0.0 { return prev; }
            let alpha = 1.0 - (-dt / tau).exp();
            prev + (sample - prev) * alpha
        }

        /// Map smoothed velocity (px/s) -> intensity factor.
        /// tune constants to taste: v_scale controls how quickly intensity falls off,
        /// min_i and max_i control range.
        #[inline]
        fn map_velocity_to_intensity(v_px_per_s: f32) -> f32 {
            // typical: small velocities < ~200 px/s => boosted intensity,
            // large velocities > ~1500 px/s => reduced intensity.
            let v = v_px_per_s.abs();
            // normalize
            let v_scale = 100.0; // px/s where intensity is near midpoint
            let t = (v / v_scale).clamp(0.0, 4.0); // normalized (0..4)
            // non-linear mapping: high intensity at small t, low intensity at big t
            let min_i = 0.1_f32;
            let max_i = 2.5_f32;
            // Use smoothstep-like curve inverted: intensity = max_i - (smoothstep) * (max_i - min_i)
            // Smoothstep approx: s = t*t*(3 - 2*t) but we scale t to [0,1] first.
            let t1 = (t / 1.5).min(1.0); // compress into 0..1 for smoothstep
            let s = t1 * t1 * (3.0 - 2.0 * t1);
            let intensity = max_i - s * (max_i - min_i);
            intensity.clamp(min_i, max_i)
        }
        
        #[inline]
        fn exp_decay(prev: f32, target: f32, rate_per_s: f32, dt: f32) -> f32 {
            if dt <= 0.0 { return prev; }
            if rate_per_s <= 0.0 { return target; }
            let factor = (-rate_per_s * dt).exp();
            target + (prev - target) * factor
        }

        let mut visible_cells: Vec<(usize, Rect)> = Vec::new();

        let pointer_pos = ui.input(|i| i.pointer.latest_pos());

        // CONFIG
        let cell_size = Vec2::new(160.0, 215.0);
        let spacing_x = 14.0_f32;
        let spacing_y = 10.0_f32;
        let frame_corner = CornerRadius::same(14);
        let base_inner_margin = 12.0_f32;
        let max_inner_margin = 22.0_f32;
        let main_animation_decay_speed = 4.0_f32; //3.0
        let rows_total: usize = self.calendar_weeks_to_show;
        let cols_per_row: usize = 7;

        self.sync_calendar_caches();

        // Time delta - this contributes to monitor refresh rate related lag if handled improperly
        let now = ui.ctx().input(|i| i.time);
        let dt = (now - self.last_anim_time).clamp(1e-8, 0.1) as f32;
        self.last_anim_time = now;

        let total_width = (cols_per_row as f32 * cell_size.x) + ((cols_per_row - 1) as f32 * spacing_x as f32);
        let daybox_width = ((total_width) / 7.5).round();

        ui.vertical(|ui| {
            egui::Grid::new("calendarday grid")
                .min_col_width(daybox_width)
                .max_col_width(daybox_width)
                .show(ui, |ui| {
                    let day_current = self.calendar_elements.iter().position(|c| c.is_today);

                    for (i, day) in WEEK_DAYS.iter().enumerate() {
                        ui.vertical_centered(|ui| {
                            if day_current.iter().any(|x| x == &i) {
                                ui.label(RichText::new(*day).strong());
                            } else {
                                ui.label(RichText::new(*day));
                            }
                        });
                    }
                });
            ui.add_space(5.0);

            // Scroll area
            egui::ScrollArea::vertical()
                .id_salt("calendar_scrollage_row_scale")
                // Restore mouse click-drag scrolling, lost when egui 0.34+ changed the ScrollArea
                // drag default to `DragScroll::OnTouch`. `ScrollSource::ALL` == the old 0.33 default.
                .scroll_source(egui::scroll_area::ScrollSource::ALL)
                .wheel_scroll_multiplier(Vec2::new(1.0, 2.0))
                .show(ui, |ui| {
                    ui.set_min_width(total_width + 20.0);
                    ui.vertical_centered(|ui| {
                        // Geometry
                        let row_height = cell_size.y + spacing_y as f32;
                        let top_y = ui.cursor().min.y;

                        // --- measure scroll delta and update smoothed velocity ---
                        let clip = ui.clip_rect();

                        // keep a smoothed_scroll_pos in self
                        let raw_scroll = clip.min.y;

                        let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);

                        let raw_velocity = if dt > 0.0 {
                            (scroll_delta) / dt
                        } else {
                            0.0
                        }.abs();
                        self.last_raw_scroll = raw_scroll;

                        // then apply exponential smoothing to speed if desired
                        self.smoothed_scroll_velocity =
                            exponential_smooth(self.smoothed_scroll_velocity, raw_velocity, self.scroll_velocity_smoothing_tau, dt);

                        // Compute animation intensity factor
                        self.animation_intensity = map_velocity_to_intensity(self.smoothed_scroll_velocity);

                        let intensity_rate = 8.0;
                        self.smoothed_animation_intensity = exp_decay(self.smoothed_animation_intensity, self.animation_intensity, intensity_rate, dt);
                        let smoothed_intensity = self.smoothed_animation_intensity;

                        let start_row_f = ((raw_scroll - top_y) / row_height).floor() as isize;
                        let end_row_f = ((clip.max.y - top_y) / row_height).ceil() as isize - 1;
                        let start_row = start_row_f.clamp(0, (rows_total as isize) - 1) as usize;
                        let end_row = end_row_f.clamp(0, (rows_total as isize) - 1) as usize;
                       
                        let visible_lo = start_row.saturating_sub(1);
                        let visible_hi = (end_row + 1).min(rows_total - 1);
                        let base_rate = main_animation_decay_speed;
                        
                        let eps = 1e-10;
                        for row in 0..rows_total {
                            if row >= visible_lo && row <= visible_hi {
                                if (self.row_anim[row] - 1.0).abs() < eps {
                                    self.row_anim[row] = 1.0;
                                } else {
                                    let delay = (row as f32 * 0.05).min(1.0);
                                    let speed_mult = 0.85 + 0.3 * (smoothed_intensity - 1.0).clamp(-1.0, 1.0);
                                    let show_rate = base_rate * (1.0 - 0.25 * delay) * speed_mult;
                                    self.row_anim[row] = exp_decay(self.row_anim[row], 1.0, show_rate, dt).clamp(0.0, 1.0);
                                }
                            } else {
                                if self.row_anim[row] < eps {
                                    self.row_anim[row] = 0.0;
                                } else {
                                    let decay_rate = base_rate * 0.6;
                                    self.row_anim[row] = exp_decay(self.row_anim[row], 0.0, decay_rate, dt).clamp(0.0, 1.0);
                                }
                            }
                        }

                        // Skip before
                        if start_row > 0 {
                            ui.add_space(row_height * start_row as f32);
                        }

                        // Visible rows
                        for row in start_row..=end_row {
                            // anim value
                            let t_avg: f32 = ease_out_quintic(self.row_anim[row].clamp(0.0, 1.0));
                            let scale_min = 0.8;

                            let eps = 1e-10;
                            
                            let scale = if t_avg < eps {
                                scale_min
                            } else if (1.0 - t_avg) < eps {
                                1.0
                            } else {
                                scale_min + (1.0 - scale_min) * t_avg
                            };

                            // Reserve layout height
                            let row_top = ui.cursor().min.y;
                            let row_rect = Rect::from_min_size(
                                Pos2::new(ui.available_rect_before_wrap().min.x, row_top),
                                Vec2::new(ui.available_width(), row_height),
                            );

                            ui.add_space(row_height);

                            let scaled_size = row_rect.size() * scale;

                            let scaled_row_rect = row_rect.shrink2(
                                (row_rect.size() - scaled_size).max(Vec2::ZERO) * 0.5
                            );

                            // Build child UI inside scaled rect
                            let mut row_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .id_salt(row)
                                    .max_rect(scaled_row_rect)
                                    .layout(Layout::left_to_right(Align::Center)),
                            );

                            // Render all cells
                            for col in 0..cols_per_row {
                                let idx = row * cols_per_row + col;
                                if idx >= self.calendar_elements.len() {
                                    row_ui.allocate_space(cell_size);
                                    row_ui.add_space(spacing_x as f32);
                                    continue;
                                }

                                let (_, rect) = row_ui.allocate_space(cell_size);

                                let animation_level = self.row_anim[row].clamp(0.0, 1.0);
                                let t = ease_out_quintic(animation_level);
                                let color_factor = ease_out_square(animation_level);

                                let minimum_fill = 5f32;
                                let normal_fill = 60f32;

                                let fill = (minimum_fill - (-normal_fill + minimum_fill) * color_factor) as u8;

                                let fill_color = Color32::from_black_alpha(fill);
                                let stroke_color = Color32::from_white_alpha(fill - 5);

                                let hovered_fill_color = Color32::from_white_alpha(fill - 5);
                                let hovered_stroke_color = Color32::from_white_alpha(fill + 40);

                                if let Some(i) = self.hovered_calendar_cell && i == idx {
                                    row_ui.painter().rect_filled(rect, frame_corner, hovered_fill_color);
                                    row_ui.painter().rect_stroke(rect, frame_corner, Stroke::new(1.5, hovered_stroke_color), StrokeKind::Outside);
                                } else {
                                    row_ui.painter().rect_filled(rect, frame_corner, fill_color);
                                    row_ui.painter().rect_stroke(rect, frame_corner, Stroke::new(1.5, stroke_color), StrokeKind::Outside);
                                }

                                let eps = 1e-11;
                                let inner_margin_f = if (t - 1.0).abs() < eps {
                                    base_inner_margin
                                } else if t < eps {
                                    max_inner_margin
                                } else {
                                    base_inner_margin + (1.0 - t) * (max_inner_margin - base_inner_margin)
                                };

                                let inner_rect = rect.shrink(inner_margin_f);

                                visible_cells.push((idx, rect));

                                let hovered = pointer_pos.map_or(false, |p| rect.contains(p));
                                if hovered {
                                    self.hovered_calendar_cell = Some(idx);
                                }

                                row_ui.scope_builder(egui::UiBuilder::new().max_rect(inner_rect), |ui| {
                                    ui.set_min_size(inner_rect.size());
                                    let cell = &self.calendar_elements[idx];
                                    let preview = &cell.preview;
                                    let is_strong = cell.is_today;
                                    let day_label = &cell.label;
                                    ui.vertical(|ui| {
                                        let num = cell.item_count;
                                        if num == 0 {
                                            ui.add(calendarwidgets::DayNumber::new(day_label, is_strong));
                                            ui.with_layout(Layout::bottom_up(Align::RIGHT), |ui| {
                                                ui.add(calendarwidgets::RotatedNumberOnly::new(day_label, is_strong));
                                            });
                                        } else if num == 1 {
                                            let first = &preview[0];
                                            ui.add(calendarwidgets::DayHeader::new(day_label, &first.name, is_strong, &first.time, preview_color(&self.active_colorscheme, first)));
                                            ui.with_layout(Layout::bottom_up(Align::RIGHT), |ui| {
                                                ui.add(calendarwidgets::RotatedNumberOnly::new(day_label, is_strong));
                                            });
                                        } else if num == 2 {
                                            let first = &preview[0];
                                            ui.add(calendarwidgets::DayHeader::new(day_label, &first.name, is_strong, &first.time, preview_color(&self.active_colorscheme, first)));
                                            let second = &preview[1];
                                            ui.add(calendarwidgets::MiddleHeader::new(&second.name, Some(&second.time), preview_color(&self.active_colorscheme, second)));
                                            ui.with_layout(Layout::bottom_up(Align::RIGHT), |ui| {
                                                ui.add(calendarwidgets::RotatedNumberOnly::new(day_label, is_strong));
                                            });
                                        } else if num == 3 {
                                            let first = &preview[0];
                                            ui.add(calendarwidgets::DayHeader::new(day_label, &first.name, is_strong, &first.time, preview_color(&self.active_colorscheme, first)));
                                            let second = &preview[1];
                                            ui.add(calendarwidgets::MiddleHeader::new(&second.name, None, preview_color(&self.active_colorscheme, second)));
                                            let third = &preview[2];
                                            ui.add(calendarwidgets::BottomHeaderRotated::new(day_label, &third.name, is_strong, &third.time, Some(&second.time), preview_color(&self.active_colorscheme, third)));
                                        } else {
                                            let first = &preview[0];
                                            ui.add(calendarwidgets::DayHeader::new(day_label, &first.name, is_strong, &first.time, preview_color(&self.active_colorscheme, first)));
                                            let second = &preview[1];
                                            ui.add(calendarwidgets::MiddleHeader::new(&second.name, None, preview_color(&self.active_colorscheme, second)));
                                            let third = &preview[2];
                                            ui.add(calendarwidgets::ButtonHeaderRotated::new(day_label, &third.name, is_strong, &third.time, Some(&second.time), preview_color(&self.active_colorscheme, third)));
                                        }
                                    });
                                });

                                // A cell is about thirteen characters to a
                                // row and a day's names are not, so a long one
                                // ends in an ellipsis however well it is
                                // fitted. Hovering is the cheap way to read the
                                // rest: the calendar is meant to be read from
                                // across the room, but when you are at the
                                // machine squinting at "Quarterly financial
                                // review…", this answers what it actually says.
                                //
                                // Registered *after* the cards so it wins the
                                // hit test against them — egui tests the most
                                // recently added widget first — and senses only
                                // hover, so the day click still belongs to the
                                // calendar's own press/drag handling.
                                if hovered && !self.any_modal_open() {
                                    // Built from the live set, not from the
                                    // cell's three-item preview: the tip exists
                                    // precisely to say what the cell had no room
                                    // for, and capping it at what the cell shows
                                    // would have it answer its own question with
                                    // "the same three, again".
                                    //
                                    // Not stored on the `DayCell` either. That
                                    // is how the day popup used to work — a
                                    // second, fully-cloned copy of every dated
                                    // item, rebuilt on every `summarize_calendar`
                                    // — and it was removed for good reason. One
                                    // scan of one hovered day costs nothing.
                                    let (lines, hidden) = {
                                        let date = self.calendar_elements[idx].date;
                                        let mut dated: Vec<&Active> = self
                                            .board.items
                                            .iter()
                                            .filter(|item| {
                                                item.deadline
                                                    .is_some_and(|when| when.date_naive() == date)
                                            })
                                            .collect();
                                        dated.sort_by_key(|item| item.deadline);
                                        let lines: Vec<(String, String)> = dated
                                            .iter()
                                            .take(CELL_TIP_MAX_ROWS)
                                            .map(|item| {
                                                (
                                                    item.deadline
                                                        .map(|when| when.format("%H:%M").to_string())
                                                        .unwrap_or_default(),
                                                    item.name.clone(),
                                                )
                                            })
                                            .collect();
                                        let hidden = dated.len().saturating_sub(lines.len());
                                        (lines, hidden)
                                    };
                                    if !lines.is_empty() {
                                        let tip = row_ui.interact(
                                            rect,
                                            egui::Id::new(("calendar_cell_tip", idx)),
                                            egui::Sense::hover(),
                                        );
                                        tip.on_hover_ui(|ui| {
                                            for (time, name) in &lines {
                                                ui.horizontal(|ui| {
                                                    if !time.is_empty() {
                                                        ui.label(
                                                            RichText::new(time)
                                                                .font(FontId::new(
                                                                    CELL_TIP_SIZE,
                                                                    FontFamily::Name("space".into()),
                                                                ))
                                                                .color(Color32::from_white_alpha(150)),
                                                        );
                                                    }
                                                    ui.label(RichText::new(name).size(CELL_TIP_SIZE));
                                                });
                                            }
                                            // Only past the tip's own ceiling,
                                            // which a personal calendar will
                                            // essentially never reach — but
                                            // leaving the rest unaccounted for
                                            // is the same silence the ellipsis
                                            // was added to break.
                                            if hidden > 0 {
                                                ui.label(
                                                    RichText::new(format!("+{hidden} more"))
                                                        .size(CELL_TIP_SIZE)
                                                        .color(Color32::from_white_alpha(130)),
                                                );
                                            }
                                        });
                                    }
                                }

                                if !self.planner_flag {
                                    if hovered {
                                        self.hovered_calendar_cell = Some(idx);
                                    } else if self.hovered_calendar_cell == Some(idx) && !hovered {
                                        self.hovered_calendar_cell = None;
                                    }
                                }

                                row_ui.add_space(spacing_x as f32);
                            } // cols

                            if let Some(Some((this, next))) = self.row_contains_month_switch.get(row) {
                                row_ui.vertical(|ui| {
                                    let font_id = FontId {
                                        size: 12.0,
                                        family: FontFamily::Name("space".into()),
                                    };

                                    let font_color = Color32::from_rgba_unmultiplied(211, 215, 211, 210);

                                    ui.label(RichText::new(this).font(font_id.clone()).color(font_color));
                                    ui.label(RichText::new("↓").font(font_id.clone()).color(font_color));
                                    ui.label(RichText::new(next).font(font_id).color(font_color));
                                });
                            }
                        } // rows

                        // After visible rows
                        if end_row + 1 < rows_total {
                            let rows_after = rows_total - (end_row + 1);
                            ui.add_space(row_height * rows_after as f32);
                        }

                        const DRAG_THRESHOLD_POINTS: f32 = 6.0;

                        if !self.any_modal_open() {
                            // Inspect input events in place rather than cloning the
                            // whole event vector every frame (B5). The closure only
                            // mutates `self` fields and reads `visible_cells`; it must
                            // not call back into `ctx`/`ui` input (would re-lock).
                            ui.ctx().input(|i| {
                                for ev in &i.events {
                                    match ev {
                                        Event::PointerButton { pos, button, pressed, .. } => {
                                            if *button == PointerButton::Primary {
                                                if *pressed {
                                                    if let Some((idx, _)) = visible_cells.iter().find(|(_, r)| r.contains(*pos)) {
                                                        self.press_origin = Some(PressState {
                                                            idx: *idx,
                                                            press_pos: *pos,
                                                            cancelled: false,
                                                        });
                                                        #[cfg(debug_assertions)] {
                                                            println!("press_origin at {}", idx);
                                                        }
                                                    } else {
                                                        self.press_origin = None;
                                                    }
                                                } else {
                                                    if let Some(press) = self.press_origin.take() {
                                                        let release_idx_opt = visible_cells.iter().find(|(_, r)| r.contains(*pos)).map(|(i, _)| *i);

                                                        let dist = press.press_pos.distance(*pos);
                                                        let moved = dist > DRAG_THRESHOLD_POINTS;

                                                        if !press.cancelled && !moved {
                                                            if let Some(release_idx) = release_idx_opt {
                                                                // Tapping a day opens the planner on it.
                                                                // A popup used to sit here listing the
                                                                // day with a "Plan day" button on the
                                                                // bottom; the planner answers the same
                                                                // question with a timeline instead of a
                                                                // list, so the tap goes straight there.
                                                                if release_idx == press.idx {
                                                                    if let Some(date) = self.calendar_elements.get(release_idx).map(|cell| cell.date) {
                                                                        self.open_planner(date);
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        },
                                        Event::PointerMoved(pos) => if let Some(press) = &mut self.press_origin {
                                            let dist = press.press_pos.distance(*pos);
                                            if dist > DRAG_THRESHOLD_POINTS {
                                                press.cancelled = true;
                                            }
                                        },
                                        Event::MouseWheel { .. } => {
                                            if let Some(press) = &mut self.press_origin {
                                                press.cancelled = true;
                                            }
                                        },
                                        _ => (),
                                    }
                                }
                            });
                        }
                        ui.add_space(2.0);
                    });
                });
        });
    }

    /* ─────────────────────────── Changing the board ─────────────────────────── */

    /// Every change to the data goes through here: the board carries the
    /// command out and saves, the calendar and task list are rebuilt so they
    /// agree with it a frame later, the phone is told, and the planner's
    /// ghosts follow the archive when the archive moved. Nothing in the UI
    /// changes an item any other way — which is what lets the same command be
    /// sent to a server instead, or queued while one is unreachable.
    fn apply(&mut self, command: board::Command) -> Result<board::Reply, board::BoardError> {
        // Stamped with this clock before anything else sees it, so the copy
        // a server gets carries the same instant this board applied.
        let now = Local::now();
        let command = command.stamped(now);
        let touches_archive = command.touches_archive();
        let touches_calendars = command.touches_calendars();
        let creates = command.creates_item();
        // The notepad is not in the calendar: its autosave rebuilds nothing.
        let only_notes = matches!(command, board::Command::SetNotes { .. });
        // Kept for the server: the replica has applied it, the server must too.
        let sent = self.sync.is_some().then(|| command.clone());
        let result = self.board.apply(command, now);
        if !only_notes {
            self.summarize_calendar();
        }
        self.phone_changed();
        if touches_archive {
            self.rebuild_planner_ghosts();
        }
        // A calendar added, removed or switched back on is fetched now, not at
        // the next tick of a ten-minute timer: nobody pastes an address and
        // then waits to find out whether it was the right one. A client has no
        // service and needs none — the server saw the same command.
        if touches_calendars
            && let Some(calendars) = &self.calendars
        {
            calendars.set_subscriptions(self.board.subscriptions().to_vec());
        }
        if let (Some(command), Some(sync), Ok(reply)) = (sent, &self.sync, &result) {
            sync.queue(command, if creates { reply.id } else { None });
        }
        result
    }

    /// Take in a freshly fetched overlay, if the calendars thread has one.
    ///
    /// Polled here rather than beside the weather in the frame because
    /// `serve_phone_requests` also runs from `App::user_event`: a fetch that
    /// lands while the window is minimized or asleep then reaches the phone at
    /// once instead of waiting for a redraw. The same reason `drain_sync_events`
    /// is called from here.
    fn adopt_calendar_overlay(&mut self) {
        let Some(calendars) = &self.calendars else { return };
        let version = calendars.version.load(Ordering::Relaxed);
        if version == self.last_calendars_version {
            return;
        }
        self.last_calendars_version = version;
        // A poisoned lock answers with an empty overlay rather than taking the
        // window down; the next fetch puts it back.
        let fresh = calendars.overlay();
        if self.board.adopt_overlay(fresh) {
            self.board.save_overlay();
            self.summarize_calendar();
            self.phone_changed();
        }
        // A calendar nobody has named takes the best name going: the one the
        // feed gives itself, or failing that its host. Through `apply` like
        // any other rename, so a server and every client hear it, and only
        // while the name is one nobody chose, so it settles.
        let renames: Vec<(u64, String)> = self
            .board
            .subscriptions()
            .iter()
            .filter_map(|s| subscriptions::better_name(s, self.board.overlay()).map(|name| (s.id, name)))
            .collect();
        for (id, name) in renames {
            self.apply_or_report(board::Command::RenameSubscription { id, name });
        }
    }

    /// Take in what the sync engine has to say: the server's board, an id
    /// that became real, a change the server refused, a change of touch.
    fn drain_sync_events(&mut self) {
        let Some(sync) = &self.sync else { return };
        let mut events = Vec::new();
        while let Ok(event) = sync.events.try_recv() {
            events.push(event);
        }
        for event in events {
            match event {
                sync::SyncEvent::Board(state) => {
                    // A board fetched before an edit made here in the same
                    // frame would undo that edit until the next round trip;
                    // with anything pending it is left, and the fetch after
                    // the send brings a board that has it.
                    if self.sync.as_ref().is_some_and(|sync| sync.status().pending > 0) {
                        continue;
                    }
                    // The server's picture wins: the replica is replaced whole,
                    // then kept on this disk for a start without the server.
                    // The archive — the one file that grows — is rewritten
                    // only when it actually differs.
                    let archive_changed = {
                        let mine = self.board.archive.entries();
                        mine.len() != state.archive.len()
                            || mine.first().map(|row| row.key()) != state.archive.first().map(|row| row.key())
                    };
                    // Notes being typed here are not written over by the
                    // server's copy of them: the debounced save sends ours in
                    // a moment, and last writer wins as everywhere else.
                    let notes = if self.should_save_textbox_text { self.board.notes.clone() } else { state.notes };
                    // The subscriptions and the overlay come in with the rest:
                    // a client never fetches for itself, so this is the only
                    // way either reaches it (§23).
                    self.board.replace(state.items, Some(state.archive), notes, state.subscriptions, state.overlay);
                    let kept = if archive_changed { self.board.save_all() } else { self.board.save_items_and_notes() };
                    if let Err(why) = kept {
                        self.show_error(format!("Could not keep a local copy of the server's board:\n{why}"));
                    }
                    self.summarize_calendar();
                    self.rebuild_planner_ghosts();
                    self.phone_changed();
                }
                sync::SyncEvent::Remapped { local, server } => {
                    // Whatever the UI was pointing at follows the item to
                    // its real id, so a block named right after it was made
                    // is still the block under the editor.
                    self.board.renumber(local, server);
                    for slot in [
                        &mut self.planner_selection,
                        &mut self.planner_naming,
                        &mut self.planner_due_edit,
                        &mut self.confirm_complete_task,
                        &mut self.confirm_delete_task,
                    ] {
                        if *slot == Some(local) {
                            *slot = Some(server);
                        }
                    }
                    if let Some(
                        PlannerDrag::Move { id, .. } | PlannerDrag::Resize { id, .. } | PlannerDrag::FromBacklog { id },
                    ) = self.planner_drag.as_mut()
                        && *id == local
                    {
                        *id = server;
                    }
                    // The task list names items by id too.
                    self.summarize_calendar();
                }
                sync::SyncEvent::Rejected { what, message } => {
                    self.show_error(format!(
                        "A change made here could not be applied on the server and was dropped — {what}:\n{message}"
                    ));
                }
                sync::SyncEvent::Online(_) => {}
            }
        }
    }

    /// `apply`, with a failure shown in the error window. The reply when it
    /// worked.
    fn apply_or_report(&mut self, command: board::Command) -> Option<board::Reply> {
        match self.apply(command) {
            Ok(reply) => Some(reply),
            Err(error) => {
                self.show_error(error.message);
                None
            }
        }
    }

    /// A task or event as the New Task / New Event dialogs make one. Returns
    /// the new item's id.
    fn add_active_thing(&mut self, name: String, deadline: Option<DateTime<Local>>, importance: Option<u8>, is_event: bool, time_importance: Option<u8>) -> Option<u64> {
        self.apply_or_report(board::Command::Add { name, deadline, importance, is_event, time_importance })
            .and_then(|reply| reply.id)
    }

    /// Drop an item from the live set without recording anything.
    ///
    /// Deliberately *not* what the delete button does — that is
    /// `retire_active_thing`, which files the item as `Dropped`. This is for
    /// the one case where there is nothing worth recording: Escape on a block
    /// you have just dragged out and not yet named. It is seconds old, the only
    /// thing in it is the slot an accident gave it, and filing "untitled,
    /// dropped a moment after it was written down" in the ledger would be
    /// noise, not history. See `discard_planner_naming`.
    fn forget_active_thing(&mut self, id: u64) {
        // A silent undo stays silent when there is nothing left to undo: the
        // block was already removed from the phone, or a server board arrived
        // without it.
        if let Err(error) = self.apply(board::Command::Forget { id })
            && error.status != 410
        {
            self.show_error(error.message);
        }
        // A removed item must not stay selected on the planner.
        if self.planner_selection == Some(id) {
            self.planner_selection = None;
            self.planner_selected_session = None;
        }
    }

    /// Seed for the task list's tie-break jitter: **today's date**.
    ///
    /// Derived rather than stored, so there is no question of when to bump it.
    /// It was a counter bumped once per `summarize_calendar` — and
    /// `summarize_calendar` runs after every mutation, so booking a block,
    /// dragging a card or nudging a deadline reshuffled the whole task list
    /// under the user while they worked. The counter was itself a fix for a
    /// clock-keyed seed that reshuffled once a second; both had the same fault,
    /// which is that they answered "how often should this move" with "whenever
    /// something happens" rather than by asking what the jitter is *for*.
    ///
    /// What it is for is keeping a task that is perpetually fourth from being
    /// permanently ignored (§14.4). That is a fairness argument, and its
    /// natural period is a day, not a keystroke. So the list is fixed for as
    /// long as you are looking at it and turns over at midnight — which is also
    /// exactly what a calendar on a wall does.
    fn shuffle_seed(&self) -> u64 {
        self.date.date_naive().num_days_from_ce() as u64
    }

    pub fn summarize_calendar(&mut self) {
        // 1) Sort and separate active things
        let (mut events, tasks): (Vec<_>, Vec<_>) = self.board.items
            .drain(..)
            .partition(|a| a.is_event);

        // Sort events by deadline. Events are expected to always carry a deadline,
        // but a hand-edited / corrupted save could violate that. Sorting on the
        // `Option` (which orders `None` first) keeps this panic-free; the per-day
        // filtering below never places a deadline-less event on the grid, and such
        // items are still retained in `board.items` rather than dropped.
        events.sort_by_key(|e| e.deadline);

        // Sort tasks by importance score, highest first. The score is evaluated
        // exactly once per task here — capturing the intended per-rebuild random
        // shuffle a single time, which also keeps the comparator consistent — and
        // the resulting `f32` is compared directly. The old code cast the score to
        // `u16`, which saturated everything above 65535 (the high-importance
        // exponential curves and the 1e9 event/broken scores) to the same value
        // and flattened their ordering.
        let now = self.date;
        let mut scored_tasks: Vec<(f32, Active)> = tasks
            .into_iter()
            .map(|t| (t.importance_score(now, self.shuffle_seed()), t))
            .collect();
        scored_tasks.sort_by(|(a, _), (b, _)| {
            b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal)
        });
        let tasks: Vec<Active> = scored_tasks.into_iter().map(|(_, t)| t).collect();

        let deadline_tasks: Vec<Active> = tasks.iter().filter(|task| task.deadline.is_some()).cloned().collect();

        // 2) Bucket dated items by day once, so each calendar cell is an O(1) map
        // lookup instead of a linear scan over every event/task (the old
        // O(days × items) rebuild). Build the buckets before moving the vecs into
        // board.items; iterating the already-sorted vecs keeps each bucket in
        // order — events by deadline, tasks by importance score (which the "take
        // 3" preview selection below relies on).
        let events_by_date = tasks::bucket_by_deadline_day(&events);
        let tasks_by_date = tasks::bucket_by_deadline_day(&deadline_tasks);

        // 3) Rebuild board.items sorted (if you need to keep the order)
        self.board.items.clear();
        self.board.items.extend(events.iter().cloned());
        self.board.items.extend(tasks);

        // 4) Determine the starting Monday
        let today = self.date;
        let monday = today
            .date_naive()
            .week(Weekday::Mon)
            .first_day();

        // What the subscribed calendars put on each day (§23), gathered once
        // rather than looked up per cell: the grid is up to ten years of them.
        // Only the calendars that are switched on, and each already carrying
        // the colour it is drawn in.
        let subscribed_by_day: HashMap<NaiveDate, Vec<PreviewItem>> = {
            let enabled: HashMap<u64, [u8; 4]> = self
                .board
                .subscriptions()
                .iter()
                .filter(|subscription| subscription.enabled)
                .map(|subscription| (subscription.id, subscription.color))
                .collect();
            let mut by_day: HashMap<NaiveDate, Vec<PreviewItem>> = HashMap::new();
            for event in &self.board.overlay().events {
                let Some(color) = enabled.get(&event.subscription).copied() else { continue };
                by_day.entry(event.day).or_default().push(PreviewItem {
                    // The glyph is the cell's only room to say "this one is
                    // not yours to tick off"; the colour says which calendar.
                    name: format!("◇ {}", event.summary),
                    time: if event.all_day {
                        String::new()
                    } else {
                        format!("{:02}:{:02}", event.start / 60, event.start % 60)
                    },
                    color_id: 0,
                    subscribed: Some(color),
                });
            }
            by_day
        };

        let mut calendar = Vec::new();

        let mut last_days_vec: Vec<Option<(String, String)>> = vec![];

        let empty: Vec<&Active> = Vec::new();

        // 5) Iterate n weeks x 7 days
        for week in 0..self.calendar_weeks_to_show {
            let mut contains_first_day_of_month = None;
            for day in 0..7 {
                let current = monday + Duration::days((week * 7 + day) as i64);

                if current.day() == 1 {
                    let prev_month = current - Duration::days(2);
                    contains_first_day_of_month = Some((prev_month.month().to_string(), current.month().to_string()));
                }

                let is_current_day: bool = current == self.date.date_naive();

                // O(1) lookups for this date's items (already ordered per bucket).
                let day_events = events_by_date.get(&current).unwrap_or(&empty);
                let day_tasks = tasks_by_date.get(&current).unwrap_or(&empty);

                // 6) Pick up to 3: events first, then tasks
                let mut chosen: Vec<&Active> = Vec::new();
                for e in day_events.iter().take(3) {
                    chosen.push(e);
                }
                if chosen.len() < 3 {
                    for t in day_tasks.iter().take(3 - chosen.len()) {
                        chosen.push(t);
                    }
                }

                // 7) Sort chosen by exact deadline time. Every item here came from
                // `day_events`/`day_tasks`, which only retain dated items, so the
                // deadline is present; format defensively regardless.
                chosen.sort_by_key(|a| a.deadline);

                let mut preview: Vec<PreviewItem> = chosen
                    .into_iter()
                    .map(|a| PreviewItem {
                        name: a.name.clone(),
                        time: a.deadline
                            .map(|d| d.format("%H:%M").to_string())
                            .unwrap_or_default(),
                        color_id: a.calendar_item_color(),
                        subscribed: None,
                    })
                    .collect();

                // Then the subscribed calendars, in whatever room is left
                // (§23). After the board's own things and never instead of
                // them: a lecture is worth knowing about, and a task is worth
                // doing. Marked with a glyph and drawn in the calendar's own
                // colour, so a cell never claims somebody else's event is
                // yours to tick off.
                let subscribed_today = subscribed_by_day.get(&current).map(Vec::as_slice).unwrap_or(&[]);
                let room = 3usize.saturating_sub(preview.len());
                preview.extend(subscribed_today.iter().take(room).cloned());

                // 8) The cell's layout is chosen by how many items land on the
                // day, so the count is all that is needed here. This used to
                // build a second, fully-cloned copy of every dated item for the
                // day popup to list; the planner that replaced the popup reads
                // `board.items` directly, so the clones are gone.
                // Counts what the cell may draw, subscribed calendars
                // included: the count is what picks the cell's layout, so a
                // preview with three things in it must not be told there is
                // one.
                let item_count = day_events.len() + day_tasks.len() + subscribed_today.len();

                calendar.push(DayCell {
                    preview,
                    item_count,
                    is_today: is_current_day,
                    date: current,
                    label: current.day().to_string(),
                });
            }
            last_days_vec.push(contains_first_day_of_month);
        }

        self.row_contains_month_switch = last_days_vec;

        self.calendar_elements = calendar;
        self.refilter_tasks();
    }

    /* ─────────────────────────── Day planner ─────────────────────────── */

    /// Open the planner on `day`, scrolled to something useful: the current hour
    /// when planning today, the start of the working day otherwise.
    fn open_planner(&mut self, day: NaiveDate) {
        self.planner_flag = true;
        self.planner_day = day;
        self.planner_selection = None;
        self.planner_selected_session = None;
        self.planner_due_edit = None;
        self.planner_drag = None;
        self.commit_planner_naming();
        self.planner_scroll_to_hour = Some(self.planner_default_scroll_hour());
        // The first thing that reads the archive on most launches: opening a
        // day asks what that day was spent on.
        self.rebuild_planner_ghosts();
    }

    /// Hour to bring into view when the day changes.
    ///
    /// Today opens an hour before now, so what's next is on screen with a little
    /// context above it — on today, "what happens next" beats "what the day
    /// starts with". Any other day opens at the start of its own contents
    /// instead, falling back to the working day when it is empty: a day whose
    /// first event is at 06:00 should not open below it.
    fn planner_default_scroll_hour(&self) -> f32 {
        if self.planner_day == self.date.date_naive() {
            return (self.date.hour() as f32 - 1.0).max(0.0);
        }

        let earliest = self
            .planner_entries()
            .iter()
            .map(|entry| entry.placement.start())
            .min();

        match earliest {
            Some(minutes) => PLANNER_DEFAULT_SCROLL_HOUR.min((minutes as f32 / 60.0 - 1.0).max(0.0)),
            None => PLANNER_DEFAULT_SCROLL_HOUR,
        }
    }

    fn planner_go_to_day(&mut self, day: NaiveDate) {
        self.planner_day = day;
        self.planner_selection = None;
        self.planner_selected_session = None;
        self.planner_due_edit = None;
        self.planner_drag = None;
        // Leaving the day keeps the title, like clicking away from the field.
        // Only Escape throws an edit away.
        self.commit_planner_naming();
        self.planner_scroll_to_hour = Some(self.planner_default_scroll_hour());
        self.rebuild_planner_ghosts();
    }

    fn close_planner(&mut self) {
        // Committing first means a half-typed title isn't silently thrown away
        // by closing the window.
        self.commit_planner_naming();
        self.planner_flag = false;
        self.planner_drag = None;
        self.planner_selection = None;
        self.planner_selected_session = None;
        self.planner_due_edit = None;
        self.planner_ghosts.clear();
    }

    /// The items that appear on `planner_day`'s timeline, in a stable order.
    ///
    /// One item can produce several entries on one day — a session per block it
    /// has here, plus its due marker if it is owed here. Each entry remembers
    /// which session it is, so a gesture on a block edits that block and no
    /// other.
    fn planner_entries(&self) -> Vec<PlannerEntry> {
        let mut entries: Vec<PlannerEntry> = Vec::new();
        for item in &self.board.items {
            for placed in planner::placements_for(item, self.planner_day) {
                entries.push(PlannerEntry {
                    id: item.id,
                    session: placed.session,
                    name: item.name.clone(),
                    color_id: item.calendar_item_color(),
                    routine: item.is_routine(),
                    placement: placed.placement,
                });
            }
        }
        // Stable, time-ordered: the lane packer sorts its own copy, but a stable
        // input order keeps egui widget ids from shuffling between frames.
        entries.sort_by_key(|e| (e.placement.start(), e.id, e.session));
        entries
    }

    /// Tasks still wanting time on a timeline — the planner's tray.
    ///
    /// A task belongs here while it has no sessions, or while its estimate is
    /// not yet covered by the sessions it has: a two-hour task with one hour
    /// booked is still half a card, and dragging it out again plans the rest
    /// (`planner::drop_length_for`).
    ///
    /// Ordered by the same score as the main task list, so the most pressing
    /// thing to schedule is at the top of its group, and split into `(due by
    /// this day, everything else)` by `planner::backlog_group`. The split is
    /// the point of the tray: "owed today and not yet planned" is the one list
    /// a day planner should lead with.
    fn planner_backlog_items(&self) -> (Vec<BacklogCard>, Vec<BacklogCard>) {
        let now = Local::now();
        let mut items: Vec<(f32, &Active)> = self
            .board.items
            .iter()
            .filter(|item| item.wants_planning())
            .map(|item| (item.importance_score(now, self.shuffle_seed()), item))
            .collect();
        items.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut due = Vec::new();
        let mut later = Vec::new();
        for (_, item) in items {
            let card = BacklogCard {
                id: item.id,
                name: item.name.clone(),
                color_id: item.calendar_item_color(),
                deadline: item.deadline,
                duration: item.duration_minutes,
                planned: item.planned_minutes(),
                hosts_name_editor: self.planner_naming == Some(item.id)
                    && !planner::appears_on(item, self.planner_day),
            };
            match planner::backlog_group(item.deadline, self.planner_day) {
                planner::BacklogGroup::Due => due.push(card),
                planner::BacklogGroup::Later => later.push(card),
            }
        }
        (due, later)
    }

    /// Move or resize one existing block on the planner's current day: session
    /// `session` of task `id`, or the event's block when `session` is `None`.
    ///
    /// An event *is* its time, so its block moves its deadline. A task's
    /// deadline is when it is **due**, which planning must never touch — the
    /// gesture edits the addressed session and nothing else.
    fn plan_item(&mut self, id: u64, session: Option<usize>, start_minutes: i32, minutes: u32) {
        let day = self.planner_day;
        self.apply_or_report(board::Command::MoveBlock { id, session, day, start: start_minutes, minutes });
    }

    /// Minutes of `day`'s booked work already behind the now-line — the
    /// masthead's "behind" figure. Zero on any day but today, which has no now.
    fn planner_behind_minutes(&self, day: NaiveDate) -> i32 {
        self.board.behind_minutes(day, Local::now())
    }

    /// Slide `day`'s remaining work past now, in order, around events and
    /// routines — the first move of §20.3. Returns how many blocks moved.
    /// Only today has a now to slide past; any other day is left alone.
    fn reflow_day(&mut self, day: NaiveDate) -> usize {
        // With this desk's own now, so a copy queued for a server slides the
        // day the way it slid here, however much later it is replayed.
        let from = planner::now_marker(day, Local::now());
        self.apply_or_report(board::Command::Reflow { day, from })
            .and_then(|reply| reply.moved)
            .unwrap_or(0)
    }

    /// Add a fresh session to a task, dropped from the tray or the footer's
    /// ＋ block button. The deadline and the estimate are untouched: this books
    /// more time, it doesn't re-describe the task. The new block is selected,
    /// ready to be dragged where it belongs.
    fn add_session(&mut self, id: u64, start_minutes: i32, minutes: u32) {
        let day = self.planner_day;
        if let Some(index) = self
            .apply_or_report(board::Command::AddBlock { id, day, start: start_minutes, minutes: Some(minutes) })
            .and_then(|reply| reply.session)
        {
            self.planner_selected_session = Some(index);
        }
    }

    /// Remove one session from a task — the block-level undo of `add_session`.
    /// Cheap and unconfirmed: the task, its deadline and its estimate all stay,
    /// and the freed time goes back onto the card in the tray.
    fn remove_session(&mut self, id: u64, session: usize) {
        self.apply_or_report(board::Command::RemoveBlock { id, session });
        // Only the selection that pointed at this task's blocks is stale. The
        // phone can remove a block of a task the desktop isn't looking at, and
        // that should not disturb whatever the desktop *is* looking at.
        if self.planner_selection == Some(id) {
            self.planner_selected_session = None;
        }
    }

    /// Set or clear a task's deadline from the footer's due editor.
    ///
    /// This is the verb whose absence forced deadlines to be *created* — as
    /// their own items, on the right day, through a mode switch — rather than
    /// simply attached to the task they describe. A dated task is scored by
    /// severity, so setting a first deadline also seeds a middling importance
    /// for the footer to adjust (`Board::apply`).
    fn set_item_deadline(&mut self, id: u64, deadline: Option<DateTime<Local>>) {
        self.apply_or_report(board::Command::SetDeadline { id, deadline });
    }

    /// Return a task to the tray whole, dropping every session but keeping the
    /// task itself, its deadline, and its estimate. Only tasks can be unplanned
    /// — an event with no time isn't an event, so its block offers delete
    /// instead.
    fn unplan_item(&mut self, id: u64) {
        self.apply_or_report(board::Command::Unplan { id });
        if self.planner_selection == Some(id) {
            self.planner_selection = None;
            self.planner_selected_session = None;
        }
    }

    /// Create an item directly on the timeline and put its title into edit mode,
    /// so putting something on a day is one gesture and a few keystrokes rather
    /// than a trip through a modal's five date combo boxes.
    ///
    /// `kind` decides which time field the gesture fills, which is the whole
    /// due-versus-planned distinction: a task gets its first session and no due
    /// date of its own — the footer's due editor adds one when there is one to
    /// add — while an event *is* its time, so the slot is its deadline.
    fn create_planned_item(&mut self, start_minutes: i32, minutes: u32, kind: planner::CreateKind) {
        let day = self.planner_day;
        let command = board::Command::Create {
            kind: board::Kind::from_create_kind(kind),
            name: String::new(),
            day,
            start: start_minutes,
            minutes,
        };
        let Some(id) = self.apply_or_report(command).and_then(|reply| reply.id) else {
            return;
        };

        let is_event = kind == planner::CreateKind::Event;
        let is_routine = kind == planner::CreateKind::Routine;
        self.planner_selection = Some(id);
        self.planner_selected_session = (!is_event && !is_routine).then_some(0);
        self.begin_planner_naming(id, true);
        // The board gave it a placeholder name; the editor starts empty so the
        // first keystroke is the name, not a correction.
        self.planner_name_input.clear();
    }

    /* ─────────────────────────── The phone view ─────────────────────────── */

    /// Something the phone shows has changed: publish the board's version so
    /// a phone waiting on `/api/wait` is answered now. Called after every
    /// `apply`, and by a colour-scheme change — which is not a change to the
    /// board, so the board is nudged first.
    fn phone_changed(&mut self) {
        self.phone_pulse.publish(self.board.version());
    }

    /// A change the phone should see that is not a change to the data — the
    /// colour scheme. Moves the version so waiters wake.
    fn phone_repaint(&mut self) {
        self.board.touch();
        self.phone_changed();
    }

    /// Serve whatever the phone has asked since the last call.
    ///
    /// Called from `App::user_event` — the phone's thread pokes the event loop
    /// after every request, so this runs even while the window is minimized or
    /// asleep — and again at the top of each frame. Each command goes through
    /// the same setter a desktop gesture uses, so the calendar, the task list
    /// and the disk agree about it a moment later, and nothing the phone does
    /// is a second way of changing a task.
    pub fn serve_phone_requests(&mut self) {
        self.drain_sync_events();
        self.adopt_calendar_overlay();
        self.tend_phone_server();
        while let Ok(request) = self.phone_rx.try_recv() {
            let reply = self.execute_phone_command(request.command);
            let _ = request.reply.send(reply);
        }
    }

    /// Bring the server in line with the setting: start it when it should be
    /// running and isn't, drop it when it shouldn't be.
    ///
    /// A restart (new port, new key, toggled off and on) cannot bind the same
    /// port until the old socket has closed, which happens on the worker
    /// thread a moment later — so for `PHONE_RESTART_WINDOW` a failed bind is
    /// retried quietly, and only after that is it reported.
    fn tend_phone_server(&mut self) {
        if !self.phone_enabled {
            self.phone_server = None;
            self.phone_retry_until = None;
            return;
        }
        if self.phone_server.is_some() {
            return;
        }
        let retrying = self.phone_retry_until.is_some_and(|until| Instant::now() < until);
        if self.phone_error.is_some() && !retrying {
            return;
        }
        if self.phone_last_attempt.is_some_and(|at| at.elapsed() < PHONE_RETRY_EVERY) {
            return;
        }
        self.phone_last_attempt = Some(Instant::now());

        // The wake pokes the event loop: `App::user_event` serves the queue,
        // even while the window is minimized or asleep.
        let proxy = self.event_proxy.clone();
        let wake: phone::Wake = Arc::new(move || {
            let _ = proxy.send_event(());
        });
        match phone::PhoneServer::start(
            &self.phone_bind,
            self.phone_port,
            self.phone_token.clone(),
            self.phone_tx.clone(),
            wake,
            Arc::clone(&self.phone_pulse),
            self.board.version(),
        ) {
            Ok(server) => {
                self.phone_server = Some(server);
                self.phone_error = None;
                self.phone_retry_until = None;
                self.refresh_phone_addresses(true);
            }
            Err(error) => {
                if !retrying {
                    self.phone_error = Some(error);
                    self.phone_retry_until = None;
                }
            }
        }
    }

    /// Stop the server and start it again with the current port and key.
    fn restart_phone_server(&mut self) {
        self.phone_server = None;
        self.phone_error = None;
        self.phone_last_attempt = None;
        self.phone_retry_until = Some(Instant::now() + PHONE_RESTART_WINDOW);
        self.tend_phone_server();
    }

    fn set_phone_port(&mut self) {
        let port = self.phone_port_input.max(phone::PORT_MIN);
        self.phone_port_input = port;
        if port == self.phone_port {
            return;
        }
        self.phone_port = port;
        self.persist_config_value("phone_server_port", port as i64);
        self.restart_phone_server();
    }

    /// Mint a new key. Every link and feed subscription made with the old one
    /// stops working, which is the point.
    fn renew_phone_token(&mut self) {
        self.phone_token = phone::generate_token();
        self.persist_config_value("phone_token", self.phone_token.clone());
        self.restart_phone_server();
    }

    fn refresh_phone_addresses(&mut self, force: bool) {
        let stale = self.phone_addresses_checked.is_none_or(|at| at.elapsed() > PHONE_ADDRESS_TTL);
        if force || stale {
            self.phone_addresses = phone::addresses_for(&self.phone_bind);
            self.phone_addresses_checked = Some(Instant::now());
        }
    }

    /// The selected colour scheme's own bytes, for the phone. Not
    /// `active_colorscheme`: `Color32` is premultiplied, and a translucent
    /// tint read back through it has its RGB scaled down by its alpha — amber
    /// arrives as brown.
    fn phone_palette(&self) -> [[u8; 4]; 6] {
        self.colorschemes
            .get(&self.selected_colorscheme_id)
            .map(|scheme| scheme.colors)
            .unwrap_or([[0; 4]; 6])
    }

    /// Answer one request from the phone: the two queries from the board as
    /// it stands, everything else through `apply` like a desktop gesture. A
    /// bad request becomes a message for the phone, not an error window at
    /// the desk — a mistyped time on the phone is the phone's to hear about.
    fn execute_phone_command(&mut self, command: phone::Command) -> phone::PhoneReply {
        if command.is_query() {
            let palette = self.phone_palette();
            // `list_tasks` is the left column exactly as drawn: filtered and
            // sorted by `summarize_calendar`, jitter and all.
            let ranked = self.list_tasks.iter().map(|task| task.id).collect();
            return phone::answer_query(&mut self.board, palette, Some(ranked), &command, Local::now());
        }
        self.apply(command).map(|reply| phone::reply_json(&reply, self.board.version()))
    }

    /// The phone view: on or off, where to point the phone, and the feed.
    fn settings_phone(&mut self, ui: &mut Ui, ctx: &Context) {
        settings_section(ui, "PHONE", "settings_phone", |ui| {
            if let Some(sync) = &self.sync {
                // This copy is a client: the phone belongs on the server,
                // which is on when this computer is not. Serving from here
                // still works — a phone edit here is queued like any other.
                let server = sync.server().to_string();
                settings_row(ui, "", |ui| {
                    settings_note(ui, format!("the board lives on {server}; point the phone at the server's own link"));
                });
            }
            settings_row(ui, "Phone view", |ui| {
                let previous = self.phone_enabled;
                ui.checkbox(
                    &mut self.phone_enabled,
                    RichText::new("Serve it while TaskDeck runs").size(SETTINGS_LABEL_SIZE),
                );
                if previous != self.phone_enabled {
                    self.persist_config_value("phone_server_enabled", self.phone_enabled);
                    self.restart_phone_server();
                }
                let status = if !self.phone_enabled {
                    "off".to_string()
                } else if self.phone_server.is_some() {
                    if self.phone_bind == phone::DEFAULT_BIND {
                        format!("on port {}", self.phone_port)
                    } else {
                        format!("on {}:{} only", phone::host_for_url(&self.phone_bind), self.phone_port)
                    }
                } else if let Some(error) = &self.phone_error {
                    error.clone()
                } else {
                    "starting…".to_string()
                };
                settings_note(ui, status);
            });

            settings_row(ui, "Port", |ui| {
                let port = ui.add(
                    egui::DragValue::new(&mut self.phone_port_input)
                        .range(phone::PORT_MIN..=u16::MAX)
                        .speed(1.0),
                );
                if port.drag_stopped() || port.lost_focus() {
                    self.set_phone_port();
                }
                if self.phone_enabled && self.phone_error.is_some() && settings_button(ui, "Try again").clicked() {
                    self.restart_phone_server();
                }
            });

            if self.phone_enabled && self.phone_server.is_some() {
                self.refresh_phone_addresses(false);
                let addresses = self.phone_addresses.clone();
                if addresses.is_empty() {
                    settings_row(ui, "Open on the phone", |ui| {
                        settings_note(ui, "no network address found — is this machine on a network?");
                    });
                } else {
                    for (index, address) in addresses.iter().enumerate() {
                        let url = phone::page_url(address, self.phone_port, &self.phone_token);
                        settings_row(ui, if index == 0 { "Open on the phone" } else { "" }, |ui| {
                            self.phone_link(ui, ctx, &url);
                            // A look at it without reaching for the phone.
                            if index == 0
                                && settings_button(ui, "Open here").clicked()
                                && let Err(error) = phone::open_in_browser(&url)
                            {
                                self.show_error(error);
                            }
                        });
                    }
                    let first = phone::page_url(&addresses[0], self.phone_port, &self.phone_token);
                    settings_row(ui, "", |ui| self.paint_phone_qr(ui, &first));
                    settings_row(ui, "", |ui| {
                        settings_note(
                            ui,
                            "same Wi-Fi, or a Tailscale address from anywhere.\nThe link is the key: share it like a password.",
                        );
                    });

                    let feed = phone::feed_url(&addresses[0], self.phone_port, &self.phone_token);
                    settings_row(ui, "Calendar feed", |ui| self.phone_link(ui, ctx, &feed));
                    settings_row(ui, "", |ui| {
                        settings_note(
                            ui,
                            "subscribe from Google Calendar or the phone's own calendar\napp — read-only; events, due dates, the plan and routines",
                        );
                    });
                }

                settings_row(ui, "Key", |ui| {
                    if settings_button(ui, "New key").clicked() {
                        self.renew_phone_token();
                    }
                    settings_note(ui, "every old link and subscription stops working");
                });
            }
        });
    }

    /// The calendars somebody else keeps (§23).
    ///
    /// Read-only everywhere they are drawn, so this is the only place they can
    /// be changed at all — and every change here is a `Command`, so a server
    /// and every other client hear about it like any other edit.
    fn settings_calendars(&mut self, ui: &mut Ui) {
        settings_section(ui, "CALENDARS", "settings_calendars", |ui| {
            let subscribed = self.board.subscriptions().to_vec();

            for subscription in &subscribed {
                let mut color = Color32::from_rgba_unmultiplied(
                    subscription.color[0],
                    subscription.color[1],
                    subscription.color[2],
                    255,
                );
                settings_row(ui, "", |ui| {
                    let mut enabled = subscription.enabled;
                    if ui.checkbox(&mut enabled, "").changed() {
                        self.apply_or_report(board::Command::SetSubscriptionEnabled {
                            id: subscription.id,
                            enabled,
                        });
                    }
                    // The colour says which calendar, not how urgent, so it is
                    // picked straight rather than off the scheme.
                    if ui.color_edit_button_srgba(&mut color).changed() {
                        self.apply_or_report(board::Command::SetSubscriptionColor {
                            id: subscription.id,
                            color: [color.r(), color.g(), color.b(), 255],
                        });
                    }
                    let mut name = subscription.name.clone();
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut name).desired_width(SETTINGS_CONTROL_WIDTH * 0.7),
                    );
                    if field.changed() {
                        // Held only while typing; the command goes on blur, so
                        // one rename is one command rather than one per key.
                        self.calendar_name_input = name.clone();
                    }
                    if field.lost_focus() && !self.calendar_name_input.trim().is_empty() {
                        let name = std::mem::take(&mut self.calendar_name_input);
                        self.apply_or_report(board::Command::RenameSubscription { id: subscription.id, name });
                    }
                    if settings_button(ui, "Remove").clicked() {
                        self.apply_or_report(board::Command::RemoveSubscription { id: subscription.id });
                    }
                });
                settings_row(ui, "", |ui| settings_note(ui, self.calendar_status_line(subscription.id)));
            }

            if subscribed.is_empty() {
                settings_row(ui, "", |ui| settings_note(ui, "nothing subscribed to yet"));
            }

            if subscribed.len() < subscriptions::SUBSCRIPTIONS_MAX {
                settings_row(ui, "Add", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.calendar_url_input)
                            .hint_text("https://…/basic.ics")
                            .desired_width(SETTINGS_CONTROL_WIDTH * 1.4),
                    );
                    if settings_button(ui, "Subscribe").clicked() {
                        let url = std::mem::take(&mut self.calendar_url_input);
                        let name = String::new();
                        match self.apply(board::Command::AddSubscription { name, url: url.clone(), color: None }) {
                            Ok(_) => self.calendar_add_error = None,
                            Err(error) => {
                                // Kept in the field so it can be corrected
                                // rather than retyped.
                                self.calendar_url_input = url;
                                self.calendar_add_error = Some(error.message);
                            }
                        }
                    }
                });
                if let Some(problem) = self.calendar_add_error.clone() {
                    settings_row(ui, "", |ui| settings_note(ui, problem));
                }
                settings_row(ui, "", |ui| {
                    settings_note(
                        ui,
                        "a secret https link from Google, Apple or a work calendar.\nRead-only: these are drawn beside your day, never edited here.",
                    );
                });
            }

            if self.calendars.is_none() {
                settings_row(ui, "", |ui| {
                    settings_note(ui, "the server reads these — this copy shows what it found");
                });
            }
        });
    }

    /// What one subscription's last fetch came to, in a line.
    fn calendar_status_line(&self, id: u64) -> String {
        match self.board.overlay().status_for(id) {
            None => "not read yet".to_string(),
            Some(status) => {
                let at = status.at.format("%H:%M");
                match &status.outcome {
                    subscriptions::FetchOutcome::Ok { events, problems, .. } if problems.is_empty() => {
                        format!("{events} events, read at {at}")
                    }
                    subscriptions::FetchOutcome::Ok { events, problems, .. } => {
                        format!("{events} events, read at {at} — {}", problems.join("; "))
                    }
                    subscriptions::FetchOutcome::Failed { why } => format!("could not read at {at}: {why}"),
                }
            }
        }
    }

    /// Where the board lives when it is not here: a `taskdeck-server`.
    ///
    /// Applied at the next start rather than live. The board is fetched, and
    /// the engine started, before there is a window — switching a running
    /// copy from its own board to a server's (or back) would mean deciding
    /// what happens to the edits in between, and a restart answers that
    /// honestly: the outbox goes with you.
    fn settings_server(&mut self, ui: &mut Ui) {
        settings_section(ui, "SERVER", "settings_server", |ui| {
            settings_row(ui, "Board on", |ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.server_url_input)
                        .hint_text("http://100.x.y.z:7373 — empty: this computer")
                        .desired_width(SETTINGS_CONTROL_WIDTH * 1.4),
                );
                if field.lost_focus() {
                    let value = self.server_url_input.trim().to_string();
                    self.server_url_input = value.clone();
                    self.persist_config_value("server_url", value);
                }
            });
            settings_row(ui, "Its key", |ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.server_token_input)
                        .password(true)
                        .hint_text("the server's phone_token")
                        .desired_width(SETTINGS_CONTROL_WIDTH * 1.4),
                );
                if field.lost_focus() {
                    let value = self.server_token_input.trim().to_string();
                    self.server_token_input = value.clone();
                    self.persist_config_value("server_token", value);
                }
            });
            settings_row(ui, "", |ui| {
                settings_note(ui, "takes effect at the next start");
                if settings_button(ui, "Restart now").clicked() {
                    self.restart_self();
                }
            });
            match &self.sync {
                Some(sync) => {
                    let status = sync.status();
                    settings_row(ui, "Now", |ui| {
                        let mut text = if status.online {
                            format!("in touch with {}", sync.server())
                        } else {
                            format!("out of touch with {}", sync.server())
                        };
                        if status.pending > 0 {
                            text.push_str(&format!(" · {} change{} waiting", status.pending, if status.pending == 1 { "" } else { "s" }));
                        }
                        settings_note(ui, text);
                    });
                    if let Some(error) = status.last_error.filter(|_| !status.online) {
                        settings_row(ui, "", |ui| settings_note(ui, error));
                    }
                    // About this disk, not the server: said here as in the
                    // menu bar, because it is what the next start would lose.
                    if let Some(problem) = status.storage_error {
                        settings_row(ui, "", |ui| settings_note(ui, problem));
                    }
                }
                None => {
                    settings_row(ui, "Now", |ui| settings_note(ui, "the board is on this computer"));
                }
            }
        });
    }

    /// The menu bar's word on the server: in touch, sending, or out of touch
    /// with changes waiting. Absent when the board is local — nothing to say.
    fn sync_indicator(&mut self, ui: &mut Ui) {
        let Some(sync) = &self.sync else { return };
        let status = sync.status();
        let (mut text, mut color) = match (status.online, status.pending) {
            (true, 0) => ("● server".to_string(), Color32::from_rgb(120, 190, 140)),
            (true, n) => (format!("↑ {n} sending…"), PLANNER_BEHIND_COLOR),
            (false, 0) => ("○ offline".to_string(), Color32::from_white_alpha(120)),
            (false, n) => (format!("○ offline · {n} waiting"), PLANNER_BEHIND_COLOR),
        };
        let mut hover = format!("The board lives on {}.", sync.server());
        if status.pending > 0 {
            hover.push_str("\nChanges made here are kept and sent when the server answers.");
        }
        if let Some(error) = status.last_error.filter(|_| !status.online) {
            hover.push_str(&format!("\n\n{error}"));
        }
        if let Some(problem) = &status.storage_error {
            // About this disk, not the network: said whether or not the
            // server is in touch, because it is what the next start loses.
            text.push_str(" · outbox not saved");
            color = PLANNER_BEHIND_COLOR;
            hover.push_str(&format!(
                "\n\n{problem}\nChanges are still sent while the server answers, but would be lost at a restart while it does not."
            ));
        }
        ui.add_space(12.0);
        ui.label(RichText::new(text).color(color)).on_hover_text(hover);
    }

    /// A link on the settings sheet: the text, wrapped to the sheet, and a
    /// button that copies it.
    fn phone_link(&self, ui: &mut Ui, ctx: &Context, url: &str) {
        let width = ui.available_width().min(SETTINGS_CONTROL_WIDTH * 1.5);
        ui.scope(|ui| {
            ui.set_max_width(width);
            ui.add(
                Label::new(
                    RichText::new(url)
                        .font(FontId::new(SETTINGS_FINE_SIZE, FontFamily::Name("space".into())))
                        .color(Color32::from_white_alpha(200)),
                )
                .wrap(),
            );
        });
        if settings_button(ui, "Copy").clicked() {
            ctx.copy_text(url.to_string());
        }
    }

    /// The phone link as a QR code, painted module by module. Cached per link:
    /// encoding is cheap, but not once a frame.
    fn paint_phone_qr(&mut self, ui: &mut Ui, url: &str) {
        if self.phone_qr.as_ref().map(|(encoded, _, _)| encoded.as_str()) != Some(url) {
            self.phone_qr = phone::qr_modules(url).map(|(width, cells)| (url.to_string(), width, cells));
        }
        let Some((_, width, cells)) = &self.phone_qr else {
            settings_note(ui, "could not draw the QR code");
            return;
        };
        let modules = width + PHONE_QR_QUIET * 2;
        // Whole points per module, so every module is the same size on screen.
        let module = (PHONE_QR_SIZE / modules as f32).floor().max(1.0);
        let side = module * modules as f32;
        let (rect, _) = ui.allocate_exact_size(vec2(side, side), egui::Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 4.0, Color32::WHITE);
        for y in 0..*width {
            for x in 0..*width {
                if cells[y * width + x] {
                    let min = rect.min + vec2((x + PHONE_QR_QUIET) as f32 * module, (y + PHONE_QR_QUIET) as f32 * module);
                    painter.rect_filled(Rect::from_min_size(min, vec2(module, module)), 0.0, Color32::BLACK);
                }
            }
        }
    }

    /// Add an undated, unplanned task from the tray's quick-add field. It lands
    /// in the tray ready to be dragged onto an hour, which is the fast way to
    /// plan: empty your head into the tray first, then place what you find.
    fn commit_planner_quick_add(&mut self) {
        let name = self.planner_quick_add_input.trim().to_string();
        if name.is_empty() {
            return;
        }
        self.planner_quick_add_input.clear();
        // Undated, with the middle horizon — the same shape the New Task
        // dialog's default makes, so where a task was typed doesn't change
        // what it is (`Board::apply`).
        self.apply_or_report(board::Command::QuickAdd { name, deadline: None });
    }

    /// Open the in-place title editor on `id`.
    ///
    /// `created` marks an item that was *just* made by the gesture opening the
    /// editor. It decides only what Escape means: for a fresh item Escape takes
    /// the item away with the edit, which is what "I dragged that out by
    /// accident" needs; for a rename it only puts the old name back.
    fn begin_planner_naming(&mut self, id: u64, created: bool) {
        self.planner_naming = Some(id);
        self.planner_naming_created = created;
        self.planner_naming_focus = true;
        self.planner_name_input = self
            .board.items
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.name.clone())
            .unwrap_or_default();
    }

    /// Keep the typed title. Enter does this, and so does clicking away from
    /// the field or leaving the day — everything except Escape.
    ///
    /// An untitled block would be a mystery rectangle, so an empty name falls
    /// back to a placeholder rather than being stored blank. The block still
    /// holds a real decision — the slot it was dragged out on — so committing
    /// it unnamed keeps that; Escape is the way to say you meant neither.
    fn commit_planner_naming(&mut self) {
        let Some(id) = self.planner_naming.take() else { return };
        let typed = self.planner_name_input.trim().to_string();
        self.planner_name_input.clear();
        self.planner_naming_created = false;

        // Nothing typed keeps what the item has — the placeholder the board
        // gave a fresh block, or the old name on a rename.
        let unchanged = typed.is_empty() || self.board.item(id).is_none_or(|item| item.name == typed);
        if unchanged {
            return;
        }
        self.apply_or_report(board::Command::Rename { id, name: typed });
    }

    /// Throw the title edit away — Escape.
    ///
    /// A rename goes back to the name the item already had. A *just-created*
    /// item goes away entirely, and without a confirmation: it is seconds old,
    /// the only thing in it is the slot an accidental drag gave it, and Escape
    /// is the key everyone reaches for to undo the last thing they did. Items
    /// any older than that are only ever deleted through the footer, which
    /// asks first.
    fn discard_planner_naming(&mut self) {
        let Some(id) = self.planner_naming.take() else { return };
        let created = std::mem::take(&mut self.planner_naming_created);
        self.planner_name_input.clear();

        if created {
            self.forget_active_thing(id);
        }
    }

    /// The planner window: the day's masthead across the top, a tray of things
    /// still wanting a slot on the left, the day's timeline on the right, and a
    /// footer that reports the load and edits whatever is selected.
    ///
    /// This is the only per-day view. Clicking a calendar day used to raise a
    /// separate popup that listed the day and offered a "Plan day" button to
    /// hand over to *this* window — two views of one day, one of which could
    /// only read it. The list is now the timeline, the popup's per-item
    /// complete/delete is the footer, its `Task+`/`Event+` buttons are the
    /// create gesture, and its day text is the masthead.
    fn show_planner(&mut self, ctx: &Context) {
        if !self.planner_flag {
            return;
        }

        // Sized from the viewport rather than fixed: the planner wants as much of
        // the day on screen at once as it can get, and the viewport is a
        // different number of points on every machine (see `apply_ui_scale`).
        //
        // The insets are what is left of the window around it — enough to show
        // that something is behind it, and no more. They were three times this,
        // and the ceilings were low enough to bind on an ordinary screen, so the
        // planner sat in the middle of a large window showing two thirds of a
        // day with a wide margin of calendar around it.
        let viewport = ctx.viewport_rect();
        let width = (viewport.width() - PLANNER_WINDOW_INSET.x).clamp(640.0, 2000.0);
        let height = (viewport.height() - PLANNER_WINDOW_INSET.y).clamp(380.0, 1500.0);

        // Captured *before* the body runs. Both editors clear their own flag
        // the instant they finish, and the keystroke that finished them is
        // still in this frame's input — so asking "is an editor open?" after
        // the fact answers "no" on exactly the frame that matters, and the
        // Enter that closed one falls straight through to the shortcut that
        // re-opens it.
        let naming_owned_frame = self.planner_naming.is_some();
        let due_editor_owned_frame = self.planner_due_edit.is_some();

        // No title bar: the masthead names the day far better than a window
        // caption could, and it is the day popup's one keepsake. Closing is the
        // masthead's own ✕, or Escape.
        egui::Window::new("day_planner")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .default_size(vec2(width, height))
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.set_height(height);

                self.planner_masthead(ui);
                ui.separator();

                // Measured rather than guessed: the masthead's height depends on
                // the font metrics of a 50-point face and on the UI scale, and a
                // constant that disagreed with either would push the footer off
                // the bottom of the window on someone else's display.
                let body_height = (ui.available_height() - PLANNER_INSPECTOR_HEIGHT - 14.0).max(160.0);

                ui.horizontal_top(|ui| {
                    self.planner_tray(ui, body_height);
                    ui.add_space(8.0);
                    self.planner_timeline(ui, body_height);
                });

                ui.separator();
                self.planner_footer(ui);
            });

        // Drawn after the planner so it stacks on top of it.
        self.show_due_editor(ctx);

        // The due editor answers Enter and Escape itself; the planner's keys
        // stand down for the whole frame it owned.
        if !due_editor_owned_frame {
            self.handle_planner_keys(ctx, naming_owned_frame);
        }

        // A drag released outside the timeline (or outside the window entirely)
        // must not leave a gesture stuck to the pointer.
        if self.planner_drag.is_some() && !ctx.input(|i| i.pointer.any_down()) {
            self.planner_drag = None;
        }
    }

    /* ───────────────────────── The keyboard ─────────────────────────
     *
     * One rule, and everything follows from it: **the topmost open thing owns
     * the keyboard.** With nothing open the menu keys below are live; open the
     * planner and its own keys take over (arrows, T, U, Del, 1–3); open
     * something over the planner and that owns them instead
     * (`modal_over_planner`). Each window closes on the key that opened it and
     * on Escape, so nothing is a one-way door.
     *
     * That is why `T` can mean "new task" out here and "today" inside the
     * planner without ambiguity: they are never live at the same time, and each
     * is the obvious mnemonic on its own screen.
     *
     * Plain letters mean a text field must never be able to see them — typing
     * "Plan the trip" into the notepad would otherwise open the planner — so
     * every one of these stands down for keyboard focus.
     */

    /// The menu bar's shortcuts: one letter per window, live only when nothing
    /// is open over the calendar.
    fn handle_menu_keys(&mut self, ctx: &Context, modal_owned_frame: bool) {
        // A focused text field owns every letter it can see: the notepad, and
        // any field inside a window that is already open.
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        // Something was open when the frame began, and it answers for itself —
        // including closing on its own letter again. `modal_owned_frame` rather
        // than a live check: the window that just closed on this very keypress
        // would otherwise be reopened by it a few lines below.
        if modal_owned_frame {
            return;
        }

        let (planner, archive, settings, task, event) = ctx.input(|i| {
            (
                i.key_pressed(Key::P),
                i.key_pressed(Key::A),
                i.key_pressed(Key::S),
                i.key_pressed(Key::T),
                i.key_pressed(Key::E),
            )
        });

        if planner {
            self.open_planner(self.planner_day);
        }
        if archive {
            self.toggle_archive();
        }
        if settings {
            self.settings_flag = true;
        }
        if task {
            self.new_task_flag = true;
            self.dialog_wants_focus = true;
        }
        if event {
            self.new_event_flag = true;
            self.dialog_wants_focus = true;
        }
    }

    /// Escape, and the letter that opened it, for the two windows that answer
    /// no keys of their own.
    ///
    /// Settings and the create dialogs used to be closable only by aiming at a
    /// button, which is a strange thing to be true of an app where every other
    /// overlay answers Escape.
    fn handle_overlay_close_keys(&mut self, ctx: &Context) {
        // The colour-scheme stack and the map picker live inside Settings and
        // have their own Cancel semantics; Escape must not pull the sheet out
        // from under them.
        if self.error_flag
            || self.color_picker_flag
            || self.coordinates_map_flag
            || self.user_wants_to_complete_task_flag
            || self.user_wants_to_delete_task_flag
        {
            return;
        }

        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let typing = ctx.egui_wants_keyboard_input();

        if self.settings_flag && (escape || (!typing && ctx.input(|i| i.key_pressed(Key::S)))) {
            self.settings_flag = false;
        }
        // Escape only, and no toggle letter: these hold a half-typed name, and
        // the letter would be part of it. Escape abandons the draft, which is
        // what Cancel does and what Escape means everywhere else in the app.
        if escape {
            self.new_task_flag = false;
            self.new_event_flag = false;
        }
    }

    /// Keyboard shortcuts for the planner.
    ///
    /// Planning a week is a lot of "next day, look, next day", so the day
    /// stepper is on the arrow keys and the whole thing can be driven without
    /// aiming at a button. Everything here is suppressed while a text field has
    /// focus — the title editor and the tray's quick-add both want plain
    /// letters and arrows for themselves.
    fn handle_planner_keys(&mut self, ctx: &Context, naming_owned_frame: bool) {
        // Something is stacked on top of the planner — a confirmation, an error,
        // the settings window. The keys belong to it, not to the day underneath.
        if self.modal_over_planner() {
            return;
        }

        // The due editor is a mode of its own: it answers Enter/Escape itself
        // (see `show_due_editor`), and stepping the day underneath it with the
        // arrow keys would be edited-task roulette.
        if self.planner_due_edit.is_some() {
            return;
        }

        // Escape backs out one level at a time: first out of a title edit —
        // throwing it away, and taking a just-created item with it — then out
        // of the planner. Closing straight from the editor would be a surprise
        // mid-sentence, so this one runs even while typing.
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            if self.planner_naming.is_some() {
                self.discard_planner_naming();
            } else {
                self.close_planner();
            }
            return;
        }

        // The title editor owns the keyboard for every frame it was open at the
        // start of: Enter belongs to it, and the letter shortcuts are letters
        // someone is typing. `naming_owned_frame` rather than a live check,
        // because Enter *commits* during the draw — by the time this runs the
        // flag is already clear, and the same Enter would re-open the editor
        // over the name just typed. (Escape is handled above, so backing out
        // still works while typing.)
        if naming_owned_frame || self.planner_naming.is_some() {
            return;
        }

        if ctx.egui_wants_keyboard_input() {
            return;
        }

        let (previous, next, today, rename, delete, unplan, close, reflow) = ctx.input(|i| {
            (
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::T),
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                i.key_pressed(Key::U),
                i.key_pressed(Key::P),
                i.key_pressed(Key::R),
            )
        });

        // The masthead's Reflow. A no-op on any day but today, and on a day
        // with nothing behind — so the key is safe to press on reflex.
        if reflow {
            self.reflow_day(self.planner_day);
        }

        // What a drag makes, on the number row — the switch is at the far right
        // of the masthead and choosing with it costs a round trip across the
        // window for something you change constantly while filling in a day.
        // In the order the switch reads, left to right.
        for (key, kind) in [
            (Key::Num1, planner::CreateKind::Task),
            (Key::Num2, planner::CreateKind::Event),
            (Key::Num3, planner::CreateKind::Routine),
        ] {
            if ctx.input(|i| i.key_pressed(key)) {
                self.planner_create_kind = kind;
            }
        }

        // The same key that opened it closes it, so `P` is a toggle from both
        // sides rather than a one-way door with Escape as the only way out.
        if close {
            self.close_planner();
            return;
        }

        if previous {
            let day = self.planner_day.pred_opt().unwrap_or(self.planner_day);
            self.planner_go_to_day(day);
        }
        if next {
            let day = self.planner_day.succ_opt().unwrap_or(self.planner_day);
            self.planner_go_to_day(day);
        }
        if today {
            self.planner_go_to_day(self.date.date_naive());
        }

        let Some(id) = self.planner_selection else { return };
        if rename {
            self.begin_planner_naming(id, false);
        }
        if unplan {
            // The same adaptive un-book as the footer's button: the clicked
            // block if one is selected, the whole plan otherwise.
            match self.planner_selected_session {
                Some(index) => self.remove_session(id, index),
                None => self.unplan_item(id),
            }
        }
        if delete {
            // Through the same confirmation the buttons raise: a keystroke is
            // easier to hit by accident than a button is.
            self.confirm_delete_task = Some(id);
            self.user_wants_to_delete_task_flag = true;
        }
    }

    /// The masthead: which day this is, how to get to another one, and what a
    /// new gesture on the timeline will make.
    ///
    /// The day text — the date in the body face over the weekday in 50-point
    /// Anton — is lifted unchanged from the calendar's day popup, negative
    /// spacing and all. It is the one piece of that window worth keeping: it
    /// names the day from across the room, which is the whole premise of an
    /// always-on calendar.
    fn planner_masthead(&mut self, ui: &mut Ui) {
        let today = self.date.date_naive();
        let (weekday, full_date) = utilities::format_date(self.planner_day);

        // One row, three groups, each free to be as tall as it needs: the row's
        // `Align::Center` puts them on a shared centre line. An earlier version
        // nudged each group down with its own `add_space` and they ended up on
        // three different heights — the arrows near the top, "Today" floating in
        // the middle of nothing, and the right-hand column lower than the date
        // it was supposed to sit beside.
        // The window frame's own margin is a few points, which put the stepper
        // and the ✕ hard into the corners of the window — a button whose edge
        // is the window's edge reads as an accident. The masthead pays for its
        // own margin instead, top and both sides.
        ui.add_space(PLANNER_EDGE_MARGIN * 0.5);
        ui.horizontal(|ui| {
            ui.add_space(PLANNER_EDGE_MARGIN);

            // The stepper stacks so it stands beside the two-line day text
            // rather than stretching the masthead.
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("◀").size(18.0)).on_hover_text("Previous day  (←)").clicked() {
                        let day = self.planner_day.pred_opt().unwrap_or(self.planner_day);
                        self.planner_go_to_day(day);
                    }
                    if ui.button(RichText::new("▶").size(18.0)).on_hover_text("Next day  (→)").clicked() {
                        let day = self.planner_day.succ_opt().unwrap_or(self.planner_day);
                        self.planner_go_to_day(day);
                    }
                });
                ui.add_space(4.0);
                let jump = ui.add_enabled(
                    self.planner_day != today,
                    Button::new(RichText::new("Today").size(PLANNER_META_SIZE)).min_size(vec2(74.0, 0.0)),
                );
                if jump.on_hover_text("Jump to today  (T)").clicked() {
                    self.planner_go_to_day(today);
                }
            });

            ui.add_space(18.0);

            // ── The day popup's headline, kept exactly as it was ──────────
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(full_date);
                    if self.planner_day == today {
                        ui.label(
                            RichText::new("· today")
                                .size(PLANNER_META_SIZE)
                                .color(Color32::from_white_alpha(150)),
                        );
                    }
                });
                ui.add_space(-9.0);
                ui.add(
                    Label::new(
                        RichText::new(weekday)
                            .font(FontId::new(PLANNER_WEEKDAY_SIZE, FontFamily::Name("anton".into()))),
                    )
                    .selectable(false),
                );
            });

            // Close, the create-kind toggle, and the day's load sit at the far
            // right, so the eye lands on the date first.
            //
            // One row, not a stacked pair. A column of two nested inside a
            // centred row does not centre as a block — egui aligns it from the
            // row's middle and it grows downwards from there, which put the
            // toggle below the bottom of the 50-point weekday it was supposed to
            // sit beside. All three fit on a line at any width the planner
            // opens at, so there is nothing to stack.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Space first: in a right-to-left row this is the gap between
                // the button and the window edge.
                ui.add_space(PLANNER_EDGE_MARGIN);
                if ui
                    .add(
                        Button::new(RichText::new("✕").size(PLANNER_META_SIZE))
                            .min_size(vec2(30.0, 30.0))
                            .corner_radius(CornerRadius::same(8)),
                    )
                    .on_hover_text("Close  (Esc)")
                    .clicked()
                {
                    self.close_planner();
                }

                ui.add_space(18.0);

                // Right-to-left, so Task reads first. The old third kind here
                // was Deadline; the footer's due editor made a due time an
                // attribute you set rather than a thing you draw, and the mode
                // went with it. Routine is a different third thing entirely —
                // not a time you record but a claim that repeats.
                ui.selectable_value(&mut self.planner_create_kind, planner::CreateKind::Routine, RichText::new("Routine").size(PLANNER_META_SIZE))
                    .on_hover_text("Time that is spoken for every week — sleep, meals, the commute  (3)");
                ui.selectable_value(&mut self.planner_create_kind, planner::CreateKind::Event, RichText::new("Event").size(PLANNER_META_SIZE))
                    .on_hover_text("Something that happens then  (2)");
                ui.selectable_value(&mut self.planner_create_kind, planner::CreateKind::Task, RichText::new("Task").size(PLANNER_META_SIZE))
                    .on_hover_text("Time set aside to work on something  (1)");
                ui.label(
                    RichText::new("New:")
                        .size(PLANNER_META_SIZE)
                        .color(Color32::from_white_alpha(150)),
                )
                .on_hover_text("What a drag makes  (1 · 2 · 3)");

                ui.add_space(24.0);

                ui.label(
                    RichText::new(self.planner_day_summary())
                        .size(PLANNER_META_SIZE)
                        .color(Color32::from_white_alpha(190)),
                )
                .on_hover_text("Overlapping blocks counted once");

                // An all-day thing off a subscribed calendar is named here
                // rather than drawn on the timeline: one drawn as a block
                // would claim the whole day and bury everything in it (§23).
                let bands = self.subscribed_all_day();
                if !bands.is_empty() {
                    ui.add_space(18.0);
                    ui.label(
                        RichText::new(bands)
                            .size(PLANNER_META_SIZE)
                            .color(Color32::from_white_alpha(140)),
                    )
                    .on_hover_text("All day, from a subscribed calendar");
                }

                // Work booked before the now-line and not ticked off is the
                // day running late, and it gets one verb: slide what is left
                // down past now, in order, around the events and routines.
                // Only today can be behind — no other day has a now.
                let behind = self.planner_behind_minutes(self.planner_day);
                if behind > 0 {
                    ui.add_space(18.0);
                    if ui
                        .add(Button::new(RichText::new("Reflow").size(PLANNER_META_SIZE)))
                        .on_hover_text("Push what is unfinished past now, in order, around events and routines  (R)")
                        .clicked()
                    {
                        self.reflow_day(self.planner_day);
                    }
                    ui.label(
                        RichText::new(format!("{} behind", planner::format_duration(behind as u32)))
                            .size(PLANNER_META_SIZE)
                            .color(PLANNER_BEHIND_COLOR),
                    )
                    .on_hover_text("Booked work that is already behind the now-line");
                }
            });
        });
    }

    /// "3h 30m planned · 4 blocks · 2 due" for the day being shown.
    fn planner_day_summary(&self) -> String {
        let entries = self.planner_entries();

        // Routines are counted apart from planned work, and this is the number
        // the whole feature exists to make honest. Folding nine hours of sleep
        // and meals into "planned" would make every day read as full; leaving
        // them out of the day altogether — which is what happened before there
        // was anywhere to put them — makes a day with four genuinely free hours
        // claim thirteen.
        let (routine, planned): (Vec<_>, Vec<_>) =
            entries.iter().partition(|entry| entry.routine);
        let placements: Vec<_> = planned.iter().map(|e| e.placement).collect();
        let summary = planner::summarize(&placements);
        let routine_placements: Vec<_> = routine.iter().map(|e| e.placement).collect();
        let routine_summary = planner::summarize(&routine_placements);

        // What the day has already been spent on, counted separately: it is
        // not "planned" any more, and adding it into that figure would make a
        // finished day look like a day with everything still ahead of it.
        let ghost_placements: Vec<_> =
            self.planner_ghosts.iter().map(|ghost| ghost.placement).collect();
        let done = planner::summarize(&ghost_placements);

        // One wording, shared with the phone view's masthead.
        planner::summary_text(summary, routine_summary, done)
    }

    /// The footer: controls for whatever is selected on the timeline, and the
    /// day's load when nothing is.
    ///
    /// The controls used to be a button strip drawn inside the block itself,
    /// which was wrong twice over: a 15-minute block has no room for three
    /// buttons, and the block's own drag target is registered over the same
    /// pixels and ate the clicks. A row of its own also gives importance
    /// somewhere to live — a task created by dragging on the timeline was
    /// previously stuck at the default with no way to change it without going
    /// through the task list.
    ///
    /// It sits below the timeline rather than above it because the row is only
    /// occupied some of the time: at the bottom, an empty one costs nothing and
    /// a full one doesn't push the day the user is aiming at.
    fn planner_footer(&mut self, ui: &mut Ui) {
        // While the title editor is open the footer says what the two keys that
        // close it will do, rather than offering controls that would take the
        // field's focus — and end the edit — the moment one was clicked. Escape
        // meaning "discard" is not guessable, so it is spelled out.
        if self.planner_naming.is_some() {
            let escape = if self.planner_naming_created {
                "Esc throws it away"
            } else {
                "Esc keeps the old name"
            };
            ui.horizontal(|ui| {
                ui.set_min_height(PLANNER_INSPECTOR_HEIGHT);
                ui.add_space(PLANNER_EDGE_MARGIN);
                ui.label(
                    RichText::new("Naming")
                        .size(PLANNER_FINE_SIZE)
                        .color(Color32::from_white_alpha(140)),
                );
                ui.label(
                    RichText::new(format!("Enter keeps it  ·  {escape}"))
                        .size(PLANNER_META_SIZE)
                        .color(Color32::from_white_alpha(190)),
                );
            });
            return;
        }

        let Some(id) = self.planner_selection else {
            self.planner_idle_footer(ui);
            return;
        };

        let Some(item) = self.board.items.iter().find(|item| item.id == id) else {
            // The selection outlived its item — completing or deleting one from
            // this very row is the usual way. Drop it and show the hints.
            self.planner_selection = None;
            self.planner_idle_footer(ui);
            return;
        };

        // A stale session index (the block was just removed or the selection
        // moved to another task) must not address someone else's session.
        let session = self
            .planner_selected_session
            .filter(|index| *index < item.sessions.len());
        self.planner_selected_session = session;

        // Copy out what the row needs; the closure below takes `&mut self`.
        let name = item.name.clone();
        let is_event = item.is_event;
        let mut recurrence = item.recurrence;
        let is_routine = recurrence.is_some();
        let is_planned = item.is_planned();
        let selected_session = session.and_then(|index| item.sessions.get(index).copied());
        let block_count = item.sessions.len();
        let planned_total = item.planned_minutes();
        let duration = item.duration_minutes;
        let deadline = item.deadline;
        let mut importance = item.importance;
        let mut time_importance = item.time_importance;

        let shown_day = self.planner_day;
        let mut complete = false;
        let mut delete = false;
        let mut unplan = false;
        let mut add_block = false;
        let mut rename = false;
        let mut changed = false;
        let mut new_duration: Option<u32> = None;
        let mut open_due_editor = false;

        ui.horizontal(|ui| {
            ui.set_min_height(PLANNER_INSPECTOR_HEIGHT);
            ui.add_space(PLANNER_EDGE_MARGIN);
            ui.label(
                RichText::new(match (is_routine, is_event) {
                    (true, _) => "Routine",
                    (_, true) => "Event",
                    _ => "Task",
                })
                .size(PLANNER_FINE_SIZE)
                .color(Color32::from_white_alpha(140)),
            );
            ui.label(RichText::new(&name).size(PLANNER_NAME_SIZE).strong());

            // When it runs: the clicked block's span, or the plan in aggregate.
            if let Some(rule) = recurrence {
                ui.label(
                    RichText::new(format!(
                        "{}–{}",
                        planner::format_minutes(rule.start_minutes),
                        planner::format_minutes(rule.start_minutes + rule.minutes as i32)
                    ))
                    .size(PLANNER_META_SIZE)
                    .color(Color32::from_white_alpha(180)),
                )
                .on_hover_text("Moving the block moves it on every day it falls on");
            } else if is_event {
                if let Some(anchor) = item_anchor_text(deadline, duration) {
                    ui.label(RichText::new(anchor).size(PLANNER_META_SIZE).color(Color32::from_white_alpha(180)));
                }
            } else if let Some(block) = selected_session {
                ui.label(
                    RichText::new(format!(
                        "{}–{}",
                        block.start.format("%H:%M"),
                        (block.start + Duration::minutes(block.minutes as i64)).format("%H:%M")
                    ))
                    .size(PLANNER_META_SIZE)
                    .color(Color32::from_white_alpha(180)),
                );
            } else if block_count > 1 {
                ui.label(
                    RichText::new(format!(
                        "{} in {} blocks",
                        planner::format_duration(planned_total),
                        block_count
                    ))
                    .size(PLANNER_META_SIZE)
                    .color(Color32::from_white_alpha(180)),
                );
            }

            // How long this takes, as a control rather than as text. For a task
            // it is the estimate — something you know about the work before you
            // know where it goes; each block's own length is set by dragging its
            // bottom edge. For an event it is simply the block's length.
            new_duration = self.planner_duration_picker(
                ui,
                recurrence.map_or(duration, |rule| Some(rule.minutes)),
                is_event || is_routine,
            );

            // The due editor's door. A planned slot is when you will *work on*
            // this; the deadline is when it is *owed* — and it is finally an
            // attribute you set, not a thing you had to create. The absence of
            // this one control is what used to force the create-a-deadline,
            // unplan, replan dance.
            if !is_event && !is_routine {
                let due_text = match deadline {
                    Some(deadline) => format!("due {} ▾", deadline.format("%a %d %b %H:%M")),
                    None => "no deadline ▾".to_string(),
                };
                let due_button = Button::new(
                    RichText::new(due_text)
                        .size(PLANNER_META_SIZE)
                        .color(if deadline.is_some() {
                            Color32::from_white_alpha(220)
                        } else {
                            Color32::from_white_alpha(130)
                        }),
                );
                if ui
                    .add(due_button)
                    .on_hover_text("When it's due. Click to change.")
                    .clicked()
                {
                    open_due_editor = true;
                }
            }

            if ui
                .button(RichText::new("✎").size(PLANNER_META_SIZE))
                .on_hover_text("Rename  (Enter, or double-click)")
                .clicked()
            {
                rename = true;
            }

            // The one knob a task's ranking asks for — and which question it is
            // depends on whether the task is dated. With a deadline, timing
            // comes from the date and the knob is *severity*: how bad is
            // missing it. Without one, the knob is the *horizon*: roughly how
            // soon this should happen. (Events take their colour from being
            // events and are ordered by time; neither question applies.)
            if is_routine {
                ui.add_space(10.0);
                if let Some(rule) = recurrence.as_mut() {
                    if planner_weekday_row(ui, rule, shown_day) {
                        changed = true;
                    }
                }
            } else if !is_event {
                ui.add_space(10.0);
                if deadline.is_some() {
                    let level = importance.get_or_insert(PLANNER_NEW_TASK_IMPORTANCE);
                    ui.label(RichText::new("Severity:").size(PLANNER_META_SIZE))
                        .on_hover_text("How bad is missing the deadline?");
                    ComboBox::from_id_salt("planner_importance")
                        .selected_text(
                            RichText::new(IMPORTANCE[(*level as usize).min(IMPORTANCE.len() - 1)])
                                .size(PLANNER_META_SIZE),
                        )
                        .show_ui(ui, |ui| {
                            for (index, label) in IMPORTANCE.iter().enumerate() {
                                if ui
                                    .selectable_value(level, index as u8, RichText::new(*label).size(PLANNER_META_SIZE))
                                    .clicked()
                                {
                                    changed = true;
                                }
                            }
                        });
                } else {
                    let level = time_importance.get_or_insert(PLANNER_NEW_TASK_HORIZON);
                    ui.label(RichText::new("Horizon:").size(PLANNER_META_SIZE))
                        .on_hover_text("How soon should this happen?");
                    ComboBox::from_id_salt("planner_horizon")
                        .selected_text(
                            RichText::new(HORIZON[(*level as usize).min(HORIZON.len() - 1)])
                                .size(PLANNER_META_SIZE),
                        )
                        .show_ui(ui, |ui| {
                            for index in HORIZON_DISPLAY_ORDER {
                                if ui
                                    .selectable_value(
                                        level,
                                        index,
                                        RichText::new(HORIZON[index as usize]).size(PLANNER_META_SIZE),
                                    )
                                    .clicked()
                                {
                                    changed = true;
                                }
                            }
                        });
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(PLANNER_EDGE_MARGIN);
                if ui
                    .button(RichText::new("✗ Delete").size(PLANNER_META_SIZE))
                    .on_hover_text("Delete  (Del)")
                    .clicked()
                {
                    delete = true;
                }
                // Only a task can be completed. Completing means "this is done,
                // file it in the archive", and an event is not work you finish
                // — it is a time that arrives and passes on its own. Offering
                // the ✓ on one asked a question with no answer, and made the
                // archive read as if the user had *done* their dentist
                // appointment. An event that shouldn't be there is deleted.
                // Only a task can be completed. A routine has no ✓ for the
                // same reason an event has none, one step further along: there
                // is no state in which sleeping every night is *done*.
                if !is_event
                    && !is_routine
                    && ui
                        .button(RichText::new("✓ Complete").size(PLANNER_META_SIZE))
                        .on_hover_text("Finish it; it goes to the archive")
                        .clicked()
                {
                    complete = true;
                }
                // One un-book button that names what it will actually do: the
                // clicked block when one is selected, the whole plan otherwise.
                if !is_event && !is_routine && is_planned {
                    let (label, hover) = if session.is_some() {
                        ("↩ Remove block", "Free this block  (U)")
                    } else {
                        ("↩ Unplan", "Free every block  (U)")
                    };
                    if ui
                        .button(RichText::new(label).size(PLANNER_META_SIZE))
                        .on_hover_text(hover)
                        .clicked()
                    {
                        unplan = true;
                    }
                }
                if !is_event
                    && !is_routine
                    && ui
                        .button(RichText::new("＋ Block").size(PLANNER_META_SIZE))
                        .on_hover_text("Another block on this day")
                        .clicked()
                {
                    add_block = true;
                }
            });
        });

        if let Some(minutes) = new_duration {
            self.set_item_duration(id, minutes);
        }

        if changed {
            // The row edited copies; say which knob moved. Importance drives
            // both the task order and the palette colour, and `apply` rebuilds
            // the calendar and task list for it.
            if let Some(rule) = recurrence {
                self.apply_or_report(board::Command::SetRepeat { id, days: rule.days, day: shown_day });
            } else if deadline.is_some() {
                if let Some(level) = importance {
                    self.apply_or_report(board::Command::SetSeverity { id, level });
                }
            } else if let Some(level) = time_importance {
                self.apply_or_report(board::Command::SetHorizon { id, level });
            }
        }
        if open_due_editor {
            self.open_due_editor(id);
        }
        if rename {
            self.begin_planner_naming(id, false);
        }
        if unplan {
            match session {
                Some(index) => self.remove_session(id, index),
                None => self.unplan_item(id),
            }
        }
        if add_block {
            self.add_block_on_shown_day(id);
        }
        if complete {
            self.confirm_complete_task = Some(id);
            self.user_wants_to_complete_task_flag = true;
        }
        if delete {
            self.confirm_delete_task = Some(id);
            self.user_wants_to_delete_task_flag = true;
        }
    }

    /// The "how long does this take" picker, returning a new length when the
    /// user chose one.
    ///
    /// For an event it is the block's length ("for"). For a task it is the
    /// **estimate** ("takes"), which is a fact about the work, not about any
    /// slot — each block's own length is set by dragging its bottom edge, and
    /// the tray card is worth the un-booked remainder of this number.
    fn planner_duration_picker(&self, ui: &mut Ui, current: Option<u32>, is_event: bool) -> Option<u32> {
        let mut chosen = None;

        ui.label(
            RichText::new(if is_event { "for" } else { "takes" })
                .size(PLANNER_FINE_SIZE)
                .color(Color32::from_white_alpha(140)),
        );

        let label = match current {
            Some(minutes) => planner::format_duration(minutes),
            None => "—".to_string(),
        };
        ComboBox::from_id_salt("planner_duration")
            .selected_text(RichText::new(label).size(PLANNER_META_SIZE))
            .show_ui(ui, |ui| {
                for minutes in planner::duration_options(current) {
                    if ui
                        .selectable_label(
                            current == Some(minutes),
                            RichText::new(planner::format_duration(minutes)).size(PLANNER_META_SIZE),
                        )
                        .clicked()
                    {
                        chosen = Some(minutes);
                    }
                }
            })
            .response
            .on_hover_text(if is_event {
                "How long it runs"
            } else {
                "How long the whole task takes"
            });

        chosen
    }

    /// Set how long an item takes, from the footer's picker: an event's block
    /// length, or a task's total estimate.
    ///
    /// For an event, legal-block clamping is the timeline's job everywhere
    /// else, so it is done here too: a length that would run the block past
    /// midnight pulls the start back rather than being silently shortened,
    /// exactly as dragging its edge would. A task's estimate is not a block, so
    /// there is nothing to clamp against the day.
    fn set_item_duration(&mut self, id: u64, minutes: u32) {
        self.apply_or_report(board::Command::SetEstimate { id, minutes });
    }

    /// Book another block for a task on the shown day, from the footer's
    /// ＋ Block button.
    ///
    /// It goes after the last block already on the day — or at the default
    /// morning hour on an empty one — for the task's remaining estimate, and
    /// lands selected, ready to be dragged where it belongs. This button exists
    /// so "split it over two days" never needs the card to leave the tray and
    /// come back.
    fn add_block_on_shown_day(&mut self, id: u64) {
        let day = self.planner_day;
        let start = self.board.next_free_start(day);
        if let Some(index) = self
            .apply_or_report(board::Command::AddBlock { id, day, start, minutes: None })
            .and_then(|reply| reply.session)
        {
            self.planner_selected_session = Some(index);
        }
    }

    /// The footer with nothing selected: what the timeline responds to.
    ///
    /// The row is reserved either way, so an empty selection costs a blank strip
    /// unless it is given something to say. The gestures are worth saying: none
    /// of drag-to-block, double-click, or drag-a-card-in announces itself.
    fn planner_idle_footer(&mut self, ui: &mut Ui) {
        let kind = match self.planner_create_kind {
            planner::CreateKind::Task => "a task",
            planner::CreateKind::Event => "an event",
            planner::CreateKind::Routine => "a routine, on this weekday",
        };

        ui.horizontal(|ui| {
            ui.set_min_height(PLANNER_INSPECTOR_HEIGHT);
            ui.add_space(PLANNER_EDGE_MARGIN);
            ui.label(
                RichText::new(format!(
                    "Drag for {kind}  ·  double-click for {}  ·  1 · 2 · 3 switch what a drag makes  \
                     ·  drag a card in from the tray  ·  click anything to edit it",
                    planner::format_duration(planner::DEFAULT_BLOCK_MINUTES)
                ))
                .size(PLANNER_META_SIZE)
                .color(Color32::from_white_alpha(130)),
            );
        });
    }

    /// Open the footer's due editor on `id`, seeding the shared date inputs
    /// from the task's current deadline — or, for a task without one, from the
    /// day being planned at the end of the working day. The seed is a
    /// suggestion in the editor, not a change to the task; nothing is written
    /// until **Set**.
    fn open_due_editor(&mut self, id: u64) {
        let deadline = self
            .board.items
            .iter()
            .find(|item| item.id == id)
            .and_then(|item| item.deadline);

        match deadline {
            Some(deadline) => {
                self.year_input = deadline.year();
                self.month_input = deadline.month() as i32;
                self.day_input = deadline.day() as i32;
                self.hour_input = deadline.hour() as i32;
                self.minute_input = deadline.minute() as i32;
            }
            None => {
                self.year_input = self.planner_day.year();
                self.month_input = self.planner_day.month() as i32;
                self.day_input = self.planner_day.day() as i32;
                self.hour_input = 17;
                self.minute_input = 0;
            }
        }
        self.planner_due_edit = Some(id);
    }

    /// The due editor: a small modal over the planner that sets, changes, or
    /// clears the selected task's deadline.
    ///
    /// This is the verb the old model was missing. A due date used to be fixed
    /// at creation — the only way to "due Friday, worked on Tuesday" was to
    /// *create* the task through a deadline gesture on Friday and then plan it
    /// from the tray, and there was no way at all to change a date later. Now
    /// it is an attribute of the task, edited where the task is.
    fn show_due_editor(&mut self, ctx: &Context) {
        let Some(id) = self.planner_due_edit else { return };
        let Some(name) = self
            .board.items
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.name.clone())
        else {
            // The task vanished under the editor (completed or deleted).
            self.planner_due_edit = None;
            return;
        };

        let mut set = false;
        let mut clear = false;
        let mut cancel = false;

        egui::Window::new("due_editor")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(format!("When is \"{name}\" due?")).size(PLANNER_NAME_SIZE));
                ui.add_space(8.0);
                self.display_date_entering(ui);
                ui.add_space(10.0);

                let (accepted, dismissed) = confirmation_keys(ui.ctx());
                ui.horizontal(|ui| {
                    if ui.add(Button::new("Set").min_size(CONFIRM_BUTTON)).clicked() || accepted {
                        set = true;
                    }
                    if ui
                        .add(Button::new("No deadline").min_size(CONFIRM_BUTTON))
                        .on_hover_text("Clear the deadline")
                        .clicked()
                    {
                        clear = true;
                    }
                    if ui.add(Button::new("Cancel").min_size(CONFIRM_BUTTON)).clicked() || dismissed {
                        cancel = true;
                    }
                    confirmation_key_hint(ui);
                });
            });

        if set {
            match utilities::parse_time_input(
                self.day_input,
                self.month_input,
                self.year_input,
                self.hour_input,
                self.minute_input,
            ) {
                Ok(date) => {
                    self.set_item_deadline(id, Some(date));
                    self.planner_due_edit = None;
                }
                Err(_) => self.show_error("Problem with date".to_string()),
            }
        }
        if clear {
            self.set_item_deadline(id, None);
            self.planner_due_edit = None;
        }
        if cancel {
            self.planner_due_edit = None;
        }
    }

    /// The tray: every task with no time set aside for it, in two groups.
    ///
    /// Cards are draggable onto the timeline, which is the whole point —
    /// planning a day should be moving things into it, not retyping them. The
    /// top group is what is *owed* by this day and still has no slot, which is
    /// the question the calendar's day popup used to answer as a flat list with
    /// nothing to do about it. Here the answer is a stack of cards to drag.
    fn planner_tray(&mut self, ui: &mut Ui, body_height: f32) {
        let (due, later) = self.planner_backlog_items();
        let today = self.date.date_naive();
        let due_label = if self.planner_day == today {
            "DUE TODAY".to_string()
        } else {
            format!("DUE BY {}", self.planner_day.format("%a %-d %b").to_string().to_uppercase())
        };

        ui.vertical(|ui| {
            ui.set_width(PLANNER_TRAY_WIDTH);

            ui.label(RichText::new("Unplanned").size(PLANNER_NAME_SIZE).strong());
            ui.label(
                RichText::new("drag onto an hour")
                    .size(PLANNER_FINE_SIZE)
                    .color(Color32::from_white_alpha(130)),
            );
            ui.add_space(6.0);

            // The quick-add sits above the list so a new task appears right
            // under the field it was typed into.
            self.planner_tray_quick_add(ui);
            ui.add_space(8.0);

            if due.is_empty() && later.is_empty() {
                ui.add_space(12.0);
                ui.label(
                    RichText::new("Nothing waiting.")
                        .size(PLANNER_META_SIZE)
                        .color(Color32::from_white_alpha(120)),
                );
                return;
            }

            let now = Local::now();
            egui::ScrollArea::vertical()
                .id_salt("planner_backlog")
                .max_height((body_height - 96.0).max(120.0))
                .show(ui, |ui| {
                    if !due.is_empty() {
                        self.planner_tray_heading(ui, &format!("{} ({})", due_label, due.len()), true);
                        for card in &due {
                            self.planner_tray_card(ui, card, now, true);
                        }
                        ui.add_space(6.0);
                    }
                    if !later.is_empty() {
                        // Only worth naming the second group when there is a
                        // first one to tell it apart from.
                        if !due.is_empty() {
                            self.planner_tray_heading(ui, &format!("BACKLOG ({})", later.len()), false);
                        }
                        for card in &later {
                            self.planner_tray_card(ui, card, now, false);
                        }
                    }
                });
        });
    }

    /// The tray's quick-add field: a name and Enter makes an undated task.
    fn planner_tray_quick_add(&mut self, ui: &mut Ui) {
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.planner_quick_add_input)
                .hint_text("+ add a task")
                .desired_width(PLANNER_TRAY_WIDTH - 16.0),
        );
        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            self.commit_planner_quick_add();
            // Keep the field hot: a brain-dump is several tasks, not one.
            response.request_focus();
        }
    }

    fn planner_tray_heading(&self, ui: &mut Ui, text: &str, urgent: bool) {
        ui.add_space(2.0);
        ui.label(
            RichText::new(text)
                .font(FontId::new(PLANNER_FINE_SIZE, FontFamily::Name("space".into())))
                .color(if urgent {
                    Color32::from_rgb(255, 150, 120)
                } else {
                    Color32::from_white_alpha(120)
                }),
        );
        ui.add_space(4.0);
    }

    /// One draggable card. `urgent` marks the "owed by this day" group, which is
    /// drawn a shade louder so the tray leads with what actually wants a slot.
    fn planner_tray_card(&mut self, ui: &mut Ui, card: &BacklogCard, now: DateTime<Local>, urgent: bool) {
        let being_dragged = matches!(
            self.planner_drag,
            Some(PlannerDrag::FromBacklog { id: dragged }) if dragged == card.id
        );
        let selected = self.planner_selection == Some(card.id);

        let accent = self.planner_accent(card.color_id);
        let frame = egui::Frame::new()
            .fill(if being_dragged {
                Color32::from_white_alpha(30)
            } else if urgent {
                Color32::from_black_alpha(90)
            } else {
                Color32::from_black_alpha(60)
            })
            .stroke(Stroke::new(if selected || urgent { 1.8 } else { 1.2 }, accent))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(10, 8));

        let response = frame
            .show(ui, |ui| {
                ui.set_width(PLANNER_TRAY_WIDTH - 40.0);
                // A task with nothing on the shown day has no block to type
                // into, so the card itself becomes the editor — otherwise
                // renaming one from the footer set the naming state and then
                // showed the field nowhere at all.
                if card.hosts_name_editor {
                    self.planner_name_field(ui, PLANNER_TRAY_WIDTH - 56.0);
                } else {
                    ui.label(RichText::new(&card.name).size(PLANNER_NAME_SIZE));
                }

                // Second line: how long it takes — and how much of that is
                // already booked, because the card is worth the *difference*
                // when dropped: a "2h · 1h booked" card lands the missing hour.
                // Then when it is owed.
                let mut meta = Vec::new();
                match (card.duration, card.planned) {
                    (Some(estimate), 0) => {
                        meta.push(format!("takes {}", planner::format_duration(estimate)));
                    }
                    (Some(estimate), booked) => {
                        meta.push(format!(
                            "takes {} · {} booked",
                            planner::format_duration(estimate),
                            planner::format_duration(booked.min(estimate))
                        ));
                    }
                    (None, _) => {}
                }
                let overdue = card.deadline.is_some_and(|deadline| deadline < now);
                if let Some(deadline) = card.deadline {
                    meta.push(format!(
                        "due {} · {}",
                        deadline.format("%a %H:%M"),
                        planner::relative_due(deadline, now)
                    ));
                }
                if !meta.is_empty() {
                    ui.label(
                        RichText::new(meta.join("  ·  "))
                            .font(FontId::new(PLANNER_FINE_SIZE, FontFamily::Name("space".into())))
                            .color(if overdue {
                                Color32::from_rgb(255, 120, 90)
                            } else {
                                Color32::from_white_alpha(170)
                            }),
                    );
                }
            })
            .response
            .interact(egui::Sense::click_and_drag());

        if response.drag_started() {
            self.planner_drag = Some(PlannerDrag::FromBacklog { id: card.id });
        }
        // Selecting from the tray puts the footer's controls — complete, delete,
        // due date, estimate, rename — on a task that has no block to click yet.
        if response.clicked() {
            self.planner_selection = Some(card.id);
            self.planner_selected_session = None;
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }

        ui.add_space(6.0);
    }

    /// The timeline: hour grid, blocks, and every pointer gesture that edits the
    /// day. Rebuilt from `board.items` each frame; the only persistent state is
    /// the in-flight drag.
    fn planner_timeline(&mut self, ui: &mut Ui, body_height: f32) {
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("planner_timeline")
            .max_height(body_height);
        if let Some(hour) = self.planner_scroll_to_hour.take() {
            scroll = scroll.vertical_scroll_offset(hour * PLANNER_HOUR_HEIGHT);
        }

        scroll.show(ui, |ui| {
            let width = ui.available_width().max(320.0);
            let geometry = planner::TimelineGeometry::new(0.0, PLANNER_HOUR_HEIGHT);

            // One allocation for the whole day. Sensing drags here (rather than
            // per-hour) is what makes "press on empty space and pull" work.
            let (rect, background) =
                ui.allocate_exact_size(vec2(width, geometry.full_height()), egui::Sense::click_and_drag());
            let geometry = planner::TimelineGeometry::new(rect.top(), PLANNER_HOUR_HEIGHT);
            let lane_area = Rect::from_min_max(
                pos2(rect.left() + PLANNER_GUTTER_WIDTH, rect.top()),
                rect.max,
            );

            self.paint_planner_grid(ui, rect, lane_area, &geometry);

            // Apply the in-flight gesture to the model *before* laying out, so
            // neighbours re-flow around the block being dragged as it moves.
            let pointer = ui.input(|i| i.pointer.interact_pos());
            let mut entries = self.planner_entries();
            if let Some(drag) = self.planner_drag.clone() {
                if let Some((start, minutes)) = self.planner_drag_preview_for(&drag, pointer, &geometry) {
                    match drag.target() {
                        // Moving or resizing an existing block: re-place that
                        // block, addressed by (task, session) so a task's other
                        // blocks stay put.
                        Some((id, session)) => {
                            if let Some(entry) = entries
                                .iter_mut()
                                .find(|e| e.id == id && e.session == session && matches!(e.placement, planner::Placement::Block { .. }))
                            {
                                entry.placement = planner::Placement::Block { start, minutes };
                            }
                        }
                        // A brand-new block — a create drag, or a tray card
                        // being dropped. It has no entry yet, so push a
                        // preview one; a dropped card previews in the task's
                        // own name and colour.
                        None => {
                            let (name, color_id) = match &drag {
                                PlannerDrag::FromBacklog { id } => self
                                    .board.items
                                    .iter()
                                    .find(|item| item.id == *id)
                                    .map(|item| (item.name.clone(), item.calendar_item_color()))
                                    .unwrap_or_default(),
                                _ => (String::new(), self.planner_new_item_color()),
                            };
                            entries.push(PlannerEntry {
                                id: u64::MAX,
                                session: None,
                                name,
                                color_id,
                                routine: self.planner_create_kind == planner::CreateKind::Routine
                                    && matches!(drag, PlannerDrag::Create { .. }),
                                placement: planner::Placement::Block { start, minutes },
                            });
                        }
                    }
                }
            }

            // Ghosts are packed *with* the live entries and then split back
            // out. One `lay_out` over both is what makes an hour you already
            // spent behave like an hour that is booked: a new block dropped on
            // top of finished work lands beside it rather than over it, exactly
            // as it would beside a live one. They are split out again because
            // the pointer handling below indexes entries and rects in step, and
            // a ghost is not something you can grab.
            let mut placements: Vec<_> = entries.iter().map(|e| e.placement).collect();
            placements.extend(self.planner_ghosts.iter().map(|ghost| ghost.placement));
            let lanes = planner::lay_out(&placements);

            let block_rects: Vec<Rect> = entries
                .iter()
                .zip(lanes.iter())
                .map(|(entry, lane)| planner_entry_rect(entry.placement, *lane, lane_area, &geometry))
                .collect();
            let ghost_rects: Vec<Rect> = self
                .planner_ghosts
                .iter()
                .zip(lanes[entries.len()..].iter())
                .map(|(ghost, lane)| planner_entry_rect(ghost.placement, *lane, lane_area, &geometry))
                .collect();

            // Interactions are registered *before* anything is drawn on top of
            // them. egui hit-tests the most recently added widget first, so a
            // control painted afterwards — the in-place title editor — wins the
            // click, instead of being swallowed by the block-sized drag target
            // covering it.
            self.handle_planner_gestures(ui, &background, &block_rects, &entries, pointer, &geometry, lane_area);

            // Exactly one entry may host the title editor. A task can put
            // several things on one day — a block per session, plus its due
            // marker — and they all carry the task's id, so "is this the item
            // being named?" does not pick one of them: every match drew its own
            // editor, three widgets deep on the same id, and egui rightly
            // complained. Prefer the block that is actually selected; fall back
            // to the item's first placement on the day.
            let naming_host = self.planner_naming.and_then(|id| {
                let selected = self.planner_selected_session;
                entries
                    .iter()
                    .position(|entry| entry.id == id && entry.session == selected)
                    .or_else(|| entries.iter().position(|entry| entry.id == id))
            });

            // Somebody else's calendar is the ground everything else sits on:
            // it is not yours to move, and it is drawn behind even the ghosts
            // so it never hides a thing this board owns (§23).
            self.paint_subscribed(ui, lane_area, &geometry);

            // Ghosts first, so anything still live sits on top of the record of
            // what is already done.
            for (ghost, ghost_rect) in self.planner_ghosts.iter().zip(ghost_rects.iter()) {
                paint_planner_ghost(ui, ghost, *ghost_rect, &self.active_colorscheme);
            }

            for (index, (entry, block_rect)) in entries.iter().zip(block_rects.iter()).enumerate() {
                self.paint_planner_entry(ui, entry, *block_rect, naming_host == Some(index));
            }
        });
    }

    /// Hour lines, hour labels, the shaded night hours, and the now-line.
    fn paint_planner_grid(
        &self,
        ui: &Ui,
        rect: Rect,
        lane_area: Rect,
        geometry: &planner::TimelineGeometry,
    ) {
        let painter = ui.painter_at(rect);

        painter.rect_filled(rect, CornerRadius::same(8), Color32::from_black_alpha(70));

        // Shade the hours most people aren't planning into, so the working day
        // reads as the foreground without hiding anything.
        for (from_hour, to_hour) in [(0.0, PLANNER_DEFAULT_SCROLL_HOUR), (22.0, 24.0)] {
            let shade = Rect::from_min_max(
                pos2(lane_area.left(), geometry.y_for(from_hour * 60.0)),
                pos2(lane_area.right(), geometry.y_for(to_hour * 60.0)),
            );
            painter.rect_filled(shade, CornerRadius::ZERO, Color32::from_black_alpha(45));
        }

        for hour in 0..=24 {
            let y = geometry.y_for(hour as f32 * 60.0);
            let on_the_hour = hour % 6 == 0;
            painter.line_segment(
                [pos2(lane_area.left(), y), pos2(lane_area.right(), y)],
                Stroke::new(
                    if on_the_hour { 1.2 } else { 0.6 },
                    Color32::from_white_alpha(if on_the_hour { 60 } else { 26 }),
                ),
            );

            if hour < 24 {
                painter.text(
                    pos2(rect.left() + PLANNER_GUTTER_WIDTH - 10.0, y + 2.0),
                    egui::Align2::RIGHT_TOP,
                    format!("{hour:02}"),
                    FontId::new(PLANNER_META_SIZE, FontFamily::Monospace),
                    Color32::from_white_alpha(if on_the_hour { 190 } else { 110 }),
                );
                // Half-hour tick, to make a 30-minute block easy to read off.
                let half = geometry.y_for(hour as f32 * 60.0 + 30.0);
                painter.line_segment(
                    [pos2(lane_area.left(), half), pos2(lane_area.left() + 12.0, half)],
                    Stroke::new(0.6, Color32::from_white_alpha(40)),
                );
            }
        }

        if let Some(minutes) = planner::now_marker(self.planner_day, self.date) {
            let y = geometry.y_for(minutes as f32);
            let now_color = Color32::from_rgb(255, 120, 90);
            painter.line_segment(
                [pos2(lane_area.left(), y), pos2(lane_area.right(), y)],
                Stroke::new(1.6, now_color),
            );
            painter.circle_filled(pos2(lane_area.left(), y), 4.0, now_color);
        }
    }

    /// Palette index a freshly created item will take, so the preview under the
    /// pointer is already the colour the committed item gets. Mirrors
    /// `Active::calendar_item_color` for the fields `create_planned_item` fills.
    fn planner_new_item_color(&self) -> usize {
        match self.planner_create_kind {
            planner::CreateKind::Event => 5,
            planner::CreateKind::Routine => tasks::ROUTINE_COLOR_INDEX,
            planner::CreateKind::Task => PLANNER_NEW_TASK_HORIZON as usize,
        }
    }

    /// Accent colour for a planner item. See `accent_for`, which the archive
    /// window shares.
    fn planner_accent(&self, color_id: usize) -> Color32 {
        accent_for(&self.active_colorscheme, color_id)
    }

    /// Rebuild the shown day's ghosts — the blocks of archived items that fall
    /// on it.
    ///
    /// Called when the day changes and whenever the archive does, rather than
    /// per frame: it walks the whole log, and the planner redraws continuously
    /// while the answer stands still. Cheap to call — on a day with nothing
    /// archived it is one pass that finds nothing.
    ///
    /// Does nothing while the planner is closed, so opening the archive on a
    /// machine that has never opened the planner does not go looking for a day.
    /// The subscribed calendars on the day being planned.
    ///
    /// Full width of the lane area rather than in a column of its own: these
    /// are not competing with the day's blocks for room, they are the ground
    /// under them. An all-day band is left off the timeline entirely — one
    /// drawn as a block would fill the day and bury everything in it — and is
    /// named in the masthead instead.
    fn paint_subscribed(&self, ui: &Ui, lane_area: Rect, geometry: &planner::TimelineGeometry) {
        let day = self.planner_day;
        let showing: Vec<(&crate::subscriptions::OverlayEvent, Color32)> = self
            .board
            .overlay()
            .events_on(day)
            .filter(|event| !event.all_day)
            .filter_map(|event| {
                let subscription =
                    self.board.subscriptions().iter().find(|s| s.id == event.subscription && s.enabled)?;
                let color =
                    Color32::from_rgb(subscription.color[0], subscription.color[1], subscription.color[2]);
                Some((event, color))
            })
            .collect();

        // Laid out among *themselves* rather than with the day's own blocks:
        // two meetings that overlap split the width between them and stay
        // readable, but they never take room away from your own work, which is
        // what they are the ground under. Drawn full width before this, which
        // put two overlapping meetings in exactly the same rectangle.
        // A background span — a course period written as a twelve-hour block —
        // takes no part in the packing and is drawn behind at full width, or
        // one long marker would squeeze every real lecture into half a column.
        let placements: Vec<planner::Placement> = showing
            .iter()
            .map(|(event, _)| planner::Placement::Block {
                start: event.start,
                minutes: if event.is_background() { 0 } else { event.minutes().max(1) as u32 },
            })
            .collect();
        let lanes = planner::lay_out(&placements);

        let mut order: Vec<usize> = (0..showing.len()).collect();
        order.sort_by_key(|index| showing.get(*index).is_some_and(|(e, _)| !e.is_background()));
        for ((event, color), (placement, lane)) in order
            .iter()
            .filter_map(|index| Some((showing.get(*index)?, (placements.get(*index)?, lanes.get(*index)?))))
        {
            let rect = if event.is_background() {
                let top = geometry.y_for(event.start as f32);
                let bottom = geometry.y_for(event.end as f32);
                Rect::from_min_max(
                    pos2(lane_area.left() + 1.0, top),
                    pos2(lane_area.right() - 1.0, bottom.max(top + 16.0)),
                )
            } else {
                planner_entry_rect(*placement, *lane, lane_area, geometry)
            };
            paint_subscribed_event(ui, rect, *color, &event.summary, &event.location, event.is_background());
        }
    }

    /// The all-day bands on the day being planned, as one line for the
    /// masthead. Empty when there are none, so the row costs nothing.
    fn subscribed_all_day(&self) -> String {
        let names: Vec<&str> = self
            .board
            .overlay()
            .events_on(self.planner_day)
            .filter(|event| event.all_day)
            .filter(|event| {
                self.board.subscriptions().iter().any(|s| s.id == event.subscription && s.enabled)
            })
            .map(|event| event.summary.as_str())
            .collect();
        names.join("  ·  ")
    }

    fn rebuild_planner_ghosts(&mut self) {
        self.planner_ghosts.clear();
        if !self.planner_flag {
            return;
        }
        self.load_archive();

        let day = self.planner_day;
        for row in self.board.archive.entries() {
            // Blocks only, and so events — which have no sessions — never
            // appear. A deleted appointment is not something the day was spent
            // on; it is something that was taken off the calendar.
            // A dropped routine must not haunt every past day it ever fell on:
            // its rule would generate a ghost for years of Tuesdays. Passing no
            // recurrence is what keeps the archive's copy of the rule out of
            // the placement.
            let placeable = planner::Placeable {
                is_event: row.is_event,
                deadline: None,
                duration_minutes: None,
                sessions: &row.sessions,
                recurrence: None,
            };
            for placed in planner::placements_of(placeable, day) {
                self.planner_ghosts.push(PlannerGhost {
                    name: row.name.clone(),
                    outcome: row.outcome,
                    color_id: row.color_id(),
                    placement: placed.placement,
                });
            }
        }
        self.planner_ghosts
            .sort_by_key(|ghost| ghost.placement.start());
    }

    /// Draw one block or due marker, plus the controls it reveals on hover.
    fn paint_planner_entry(&mut self, ui: &mut Ui, entry: &PlannerEntry, rect: Rect, naming: bool) {
        let palette = self.active_colorscheme[entry.color_id.min(5)];
        let accent = self.planner_accent(entry.color_id);
        // The strong highlight follows the *clicked block*: a task planned
        // twice on one day lights up only the session the footer's controls
        // would act on. Its sibling blocks and its due marker still read as
        // selected, one notch quieter.
        let selected = self.planner_selection == Some(entry.id)
            && (self.planner_selected_session.is_none()
                || self.planner_selected_session == entry.session
                || entry.session.is_none());

        // Text is clipped to the item it belongs to. An item only gets a share of
        // the width when something overlaps it, and a long title spilling out of
        // its column lands on top of whatever it is sharing the hour with —
        // which is exactly the double-booking the side-by-side layout exists to
        // make readable.
        let clipped = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));

        match entry.placement {
            planner::Placement::Marker { at, due } => {
                let painter = ui.painter();
                // A due marker is deliberately flatter than a block: it marks a
                // line in the day rather than claiming time in it.
                painter.rect_filled(rect, CornerRadius::same(6), Color32::from_black_alpha(150));
                if !due {
                    painter.rect_filled(rect, CornerRadius::same(6), palette);
                }
                painter.rect_stroke(
                    rect,
                    CornerRadius::same(6),
                    Stroke::new(if selected { 2.0 } else { 1.0 }, accent),
                    StrokeKind::Inside,
                );
                let prefix = format!("{}  {}", planner::format_minutes(at), if due { "due · " } else { "" });
                clipped.text(
                    pos2(rect.left() + 8.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    if naming { prefix.clone() } else { format!("{prefix}{}", entry.name) },
                    FontId::new(PLANNER_META_SIZE, FontFamily::Monospace),
                    Color32::from_white_alpha(220),
                );

                // A marker created on the timeline — a deadline dropped onto the
                // day — needs naming just as a block does, and a marker is too
                // short to hold a text field inside it. Hang the editor off the
                // end of the time prefix, level with the moment it marks, and
                // keep a usable width for it however narrow the column is.
                if naming {
                    let after_prefix = rect.left() + 16.0 + prefix.len() as f32 * 7.0;
                    let field = Rect::from_min_max(
                        pos2(after_prefix.min(rect.right() - 90.0).max(rect.left()), rect.center().y - 13.0),
                        pos2(rect.right() - 4.0, rect.center().y + 13.0),
                    );
                    self.planner_name_editor(ui, field);
                }
            }
            planner::Placement::Block { start, minutes } => {
                let painter = ui.painter();
                // Base first, palette over it: the base guarantees the block is
                // legible under any scheme (including the transparent default),
                // and the palette still tints it the same colour the calendar
                // uses for this item.
                //
                // A routine is drawn a step back from all of that. It is the
                // backdrop the day is planned *around* — eight hours of sleep
                // rendered as loud as an hour of actual work would make every
                // day look full of nothing. Quieter fill, thinner outline; it
                // is still solid, because unlike a ghost it is live and you can
                // pick it up.
                let recessive = entry.routine;
                painter.rect_filled(
                    rect,
                    CornerRadius::same(8),
                    Color32::from_black_alpha(if recessive { 110 } else { 170 }),
                );
                painter.rect_filled(
                    rect,
                    CornerRadius::same(8),
                    if recessive { palette.gamma_multiply(0.45) } else { palette },
                );
                painter.rect_stroke(
                    rect,
                    CornerRadius::same(8),
                    Stroke::new(
                        if selected { 2.2 } else { 1.2 },
                        match (selected, recessive) {
                            (true, _) => Color32::WHITE,
                            (false, true) => accent.gamma_multiply(0.6),
                            (false, false) => accent,
                        },
                    ),
                    StrokeKind::Inside,
                );

                let text_rect = rect.shrink2(vec2(8.0, 5.0));
                clipped.text(
                    text_rect.left_top(),
                    egui::Align2::LEFT_TOP,
                    format!(
                        "{}{}–{}  ·  {}",
                        // Says the block is a rule, not a one-off, right where
                        // the gesture that would move it every week happens.
                        if recessive { "↻ " } else { "" },
                        planner::format_minutes(start),
                        planner::format_minutes(start + minutes as i32),
                        planner::format_duration(minutes)
                    ),
                    FontId::new(PLANNER_FINE_SIZE, FontFamily::Monospace),
                    Color32::from_white_alpha(if recessive { 155 } else { 200 }),
                );

                if naming {
                    // Type the title straight into the block.
                    let field = Rect::from_min_max(
                        pos2(text_rect.left(), text_rect.top() + 15.0),
                        pos2(text_rect.right(), (text_rect.top() + 41.0).min(text_rect.bottom())),
                    );
                    self.planner_name_editor(ui, field);
                } else if rect.height() > PLANNER_TWO_LINE_BLOCK {
                    clipped.text(
                        pos2(text_rect.left(), text_rect.top() + 16.0),
                        egui::Align2::LEFT_TOP,
                        &entry.name,
                        FontId::new(PLANNER_NAME_SIZE, FontFamily::Monospace),
                        Color32::WHITE,
                    );
                } else {
                    // Too short for two lines: title only, on the same row.
                    clipped.text(
                        pos2(text_rect.right(), text_rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        &entry.name,
                        FontId::new(PLANNER_META_SIZE, FontFamily::Monospace),
                        Color32::WHITE,
                    );
                }

                // The resize grip. A single faint hairline read as decoration
                // rather than as a handle, which is most of why nobody found
                // it: two stacked bars centred in a shaded strip look like
                // something you pull, and the strip is the size of the target
                // you actually have to hit. It brightens under the pointer.
                if rect.height() >= 24.0 {
                    let strip = Rect::from_min_max(
                        pos2(rect.left() + 1.0, rect.bottom() - PLANNER_RESIZE_HANDLE),
                        pos2(rect.right() - 1.0, rect.bottom() - 1.0),
                    );
                    let hot = ui.rect_contains_pointer(strip);
                    let painter = ui.painter();
                    painter.rect_filled(
                        strip,
                        CornerRadius { nw: 0, ne: 0, sw: 7, se: 7 },
                        Color32::from_white_alpha(if hot { 34 } else { 12 }),
                    );
                    let bars = Color32::from_white_alpha(if hot { 235 } else { 165 });
                    let half = (rect.width() * 0.16).clamp(11.0, 26.0);
                    for offset in [-2.5_f32, 1.5] {
                        let y = strip.center().y + offset;
                        painter.line_segment(
                            [pos2(rect.center().x - half, y), pos2(rect.center().x + half, y)],
                            Stroke::new(1.6, bars),
                        );
                    }
                }
            }
        }

    }

    /// The in-place title editor, drawn into `field` on top of whatever it
    /// belongs to. Shared by blocks and markers so naming a deadline works the
    /// same way naming a block does, and Enter means the same thing in both.
    /// The title field itself. Both hosts use it — the block on the timeline
    /// and, for a task with nothing on the shown day, its card in the tray — so
    /// Enter and Escape mean the same thing wherever the editor happens to be.
    ///
    /// Only ever called once per frame: see `naming_host` in `planner_timeline`
    /// for who gets to call it.
    fn planner_name_field(&mut self, ui: &mut Ui, width: f32) {
        let Some(id) = self.planner_naming else { return };

        let response = ui.add(
            egui::TextEdit::singleline(&mut self.planner_name_input)
                // Keyed to the item, not to where the field happens to sit.
                // The block moves as neighbours re-flow around it, and an id
                // derived from its position would change with it — dropping
                // focus, and with it the edit, mid-word.
                .id(egui::Id::new(("planner_name_editor", id)))
                .hint_text("name it")
                .desired_width(width),
        );

        // Claim focus once, when the editor appears. This used to run every
        // frame, which is why Enter did nothing: `lost_focus` is a live query
        // into egui's focus memory, so putting focus straight back after Enter
        // made the field surrender it meant the query below always answered
        // "no" and the edit could never be finished.
        if self.planner_naming_focus {
            response.request_focus();
            self.planner_naming_focus = false;
        }

        // Enter ends the edit, and so does clicking anything else — both are
        // the field losing focus, and both mean "keep this". Escape drops focus
        // too but means the opposite, and `handle_planner_keys` has it; this
        // must not race it to the commit.
        if response.lost_focus() && !ui.input(|i| i.key_pressed(Key::Escape)) {
            let by_enter = ui.input(|i| i.key_pressed(Key::Enter));
            let created = self.planner_naming_created;
            self.commit_planner_naming();

            // Enter on a *just-made* item ends the whole gesture — drag it out,
            // name it, done — so the selection is dropped with the editor and
            // the footer goes back to the day. Leaving it selected left a block
            // lit up and a row of controls aimed at it that nobody asked for.
            //
            // Only for Enter, and only for a new item. A rename keeps its
            // selection (you picked that item deliberately), and losing focus by
            // *clicking* must not clear it: the click has already chosen what to
            // select, and this runs afterwards.
            if by_enter && created {
                self.planner_selection = None;
                self.planner_selected_session = None;
            }
        }
    }

    /// The title field laid over a block or marker on the timeline.
    fn planner_name_editor(&mut self, ui: &mut Ui, field: Rect) {
        ui.scope_builder(egui::UiBuilder::new().max_rect(field), |ui| {
            self.planner_name_field(ui, field.width());
        });
    }

    /// Start, track, and commit the timeline gestures.
    fn handle_planner_gestures(
        &mut self,
        ui: &mut Ui,
        background: &egui::Response,
        block_rects: &[Rect],
        entries: &[PlannerEntry],
        pointer: Option<Pos2>,
        geometry: &planner::TimelineGeometry,
        lane_area: Rect,
    ) {
        // --- start a gesture -------------------------------------------------
        if self.planner_drag.is_none() {
            for (entry, rect) in entries.iter().zip(block_rects.iter()) {
                // The block being named has a text field inside it. These
                // interaction rects are registered after the field is drawn, so
                // they sit on top of it and would swallow every click meant for
                // the cursor — leave the block alone until the title is done.
                if self.planner_naming == Some(entry.id) {
                    continue;
                }
                // Every interaction id carries the session, so a task planned
                // twice on one day is two independently grabbable blocks.
                let widget_key = (entry.id, entry.session);

                let planner::Placement::Block { start, minutes } = entry.placement else {
                    // Due markers are not draggable: a deadline is a fact about
                    // the task, and rewriting it belongs to the footer's due
                    // editor, deliberately. Click still selects.
                    let response = ui.interact(*rect, egui::Id::new(("planner_marker", widget_key)), egui::Sense::click());
                    if response.clicked() {
                        self.planner_selection = Some(entry.id);
                        self.planner_selected_session = None;
                    }
                    continue;
                };

                // Body first, resize handle second. The handle sits *inside* the
                // body's rect, and egui gives a click to the most recently added
                // widget under the pointer — so registering the handle first, as
                // this once did, made it unreachable: every press on it was a
                // press on the body. When two interaction rects overlap, the one
                // that must win goes last.
                let body_response =
                    ui.interact(*rect, egui::Id::new(("planner_block", widget_key)), egui::Sense::click_and_drag());

                let handle = Rect::from_min_max(
                    pos2(rect.left(), rect.bottom() - PLANNER_RESIZE_HANDLE),
                    rect.max,
                );
                let handle_response =
                    ui.interact(handle, egui::Id::new(("planner_resize", widget_key)), egui::Sense::click_and_drag());

                if handle_response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                } else if body_response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }

                let select = |app: &mut Self| {
                    app.planner_selection = Some(entry.id);
                    app.planner_selected_session = entry.session;
                };

                if handle_response.drag_started() {
                    self.planner_drag = Some(PlannerDrag::Resize {
                        id: entry.id,
                        session: entry.session,
                        start,
                    });
                    select(self);
                    continue;
                }
                // A click on the handle that never became a drag is still a
                // click on the block: selecting by aiming slightly low should
                // not be a way to select nothing.
                if handle_response.clicked() || body_response.clicked() {
                    select(self);
                }
                if body_response.double_clicked() {
                    select(self);
                    self.begin_planner_naming(entry.id, false);
                }
                if body_response.drag_started() {
                    let grab = pointer
                        .map(|p| (geometry.minutes_at(p.y) - start as f32).round() as i32)
                        .unwrap_or(0)
                        .clamp(0, minutes as i32);
                    self.planner_drag = Some(PlannerDrag::Move {
                        id: entry.id,
                        session: entry.session,
                        grab_offset: grab,
                        minutes,
                    });
                    select(self);
                }
            }

            // Empty timeline: pressing and pulling blocks out new time.
            if self.planner_drag.is_none() && background.drag_started() {
                if let Some(pos) = pointer {
                    let on_a_block = block_rects.iter().any(|rect| rect.contains(pos));
                    if !on_a_block && pos.x >= lane_area.left() {
                        self.commit_planner_naming();
                        self.planner_drag = Some(PlannerDrag::Create {
                            anchor: planner::snap(geometry.minutes_at(pos.y)),
                        });
                    }
                }
            }

            // A click on bare timeline clears the selection and any title edit.
            if background.clicked() {
                if let Some(pos) = pointer {
                    if !block_rects.iter().any(|rect| rect.contains(pos)) {
                        self.commit_planner_naming();
                        self.planner_selection = None;
                        self.planner_selected_session = None;
                    }
                }
            }

            // Double-clicking bare timeline makes the same thing a drag would,
            // at the default length. Most of what goes on a day is half an hour
            // of something, and aiming a precise drag for it is work the app can
            // do instead. (The first click of the pair has already cleared the
            // selection above; creating sets it again.)
            if background.double_clicked() {
                if let Some(pos) = pointer {
                    let on_a_block = block_rects.iter().any(|rect| rect.contains(pos));
                    if !on_a_block && pos.x >= lane_area.left() {
                        let start = planner::snap(geometry.minutes_at(pos.y));
                        let kind = self.planner_create_kind;
                        self.create_planned_item(start, planner::DEFAULT_BLOCK_MINUTES, kind);
                    }
                }
            }
        }

        // --- commit on release ----------------------------------------------
        let released = !ui.input(|i| i.pointer.any_down());
        if !released {
            return;
        }
        let Some(drag) = self.planner_drag.take() else { return };
        let Some((start, minutes)) = self.planner_drag_preview_for(&drag, pointer, geometry) else {
            return;
        };

        // Dropping outside the lanes (on the hour gutter, or off the window) is
        // a cancel, not a plan at 00:00.
        let dropped_in_lanes = pointer.is_some_and(|p| {
            p.x >= lane_area.left() && p.x <= lane_area.right() && lane_area.y_range().contains(p.y)
        });

        match drag {
            PlannerDrag::Create { .. } => {
                if dropped_in_lanes {
                    let kind = self.planner_create_kind;
                    self.create_planned_item(start, minutes, kind);
                }
            }
            PlannerDrag::Move { id, session, .. } | PlannerDrag::Resize { id, session, .. } => {
                self.plan_item(id, session, start, minutes);
            }
            PlannerDrag::FromBacklog { id } => {
                if dropped_in_lanes {
                    // A drop from the tray *adds* a session — the card may
                    // already have time booked elsewhere.
                    self.add_session(id, start, minutes);
                    self.planner_selection = Some(id);
                }
            }
        }
    }

    /// Where the in-flight gesture currently puts its block, as
    /// `(start, minutes)`; which block that is, the gesture itself knows
    /// (`Drag::target`, or a new pending one).
    ///
    /// The arithmetic lives in `planner::preview`, which is unit-tested; this
    /// only supplies the two things it can't know: the pointer's position in
    /// minutes, and the length to give a tray card being dropped (the task's
    /// unplanned remainder, else the default).
    ///
    /// Taking the gesture as an argument rather than reading `self.planner_drag`
    /// lets the commit path call this *after* `take()`ing the gesture, so the
    /// live preview and the committed value come from one place and cannot
    /// disagree about where the block landed.
    fn planner_drag_preview_for(
        &self,
        drag: &PlannerDrag,
        pointer: Option<Pos2>,
        geometry: &planner::TimelineGeometry,
    ) -> Option<(i32, u32)> {
        let minutes_at_pointer = geometry.minutes_at(pointer?.y);

        let default_minutes = match drag {
            PlannerDrag::FromBacklog { id } => self
                .board.items
                .iter()
                .find(|item| item.id == *id)
                .map(planner::drop_length_for)
                .unwrap_or(planner::DEFAULT_BLOCK_MINUTES),
            _ => planner::DEFAULT_BLOCK_MINUTES,
        };

        Some(planner::preview(drag, minutes_at_pointer, default_minutes))
    }

    /// Show an error.
    ///
    /// A second error raised before the first has been read is **appended
    /// under it**, not written over it. There is one error window, and an
    /// unread error that quietly vanished — a failed save replaced by the
    /// failed config write that followed it — is the worst kind, because the
    /// first one is usually the explanation of the second. Bounded, so a
    /// failure that repeats every frame cannot grow the window past the
    /// screen; and the same message twice in a row is shown once.
    fn show_error(&mut self, errortext: String) {
        if self.error_flag && !self.error_text.is_empty() {
            if self.error_text.ends_with(&errortext) {
                return;
            }
            if self.error_text.len() >= ERROR_TEXT_CAP {
                if !self.error_text.ends_with(ERROR_TEXT_MORE) {
                    self.error_text.push_str(ERROR_TEXT_MORE);
                }
                return;
            }
            self.error_text.push_str("\n\n");
            self.error_text.push_str(&errortext);
            return;
        }
        self.error_flag = true;
        self.error_text = errortext;
    }

    /// True when any modal/overlay window is open. This is the single source of
    /// truth for "is the calendar covered" — used both to suppress the calendar
    /// tap/drag machine and to clear the hovered cell. It replaces two
    /// hand-maintained flag disjunctions that had drifted apart and *both* missed
    /// the map picker and colour-scheme editor (so calendar input leaked behind
    /// them). Adding a future modal means adding one line to `modal_over_planner`.
    fn any_modal_open(&self) -> bool {
        self.planner_flag || self.modal_over_planner()
    }

    /// The same list minus the planner itself — that is, "is something stacked
    /// on top of the planner". The planner's keyboard shortcuts stand down for
    /// it: a confirmation dialog raised from the footer must not have the day
    /// step out from under it when the user reaches for an arrow key, and Escape
    /// belongs to the topmost window.
    fn modal_over_planner(&self) -> bool {
        self.new_task_flag
            || self.new_event_flag
            || self.settings_flag
            || self.archive_view.open
            || self.error_flag
            || self.user_wants_to_complete_task_flag
            || self.user_wants_to_delete_task_flag
            || self.coordinates_map_flag
            || self.color_picker_flag
            || self.edit_colorscheme_flag
            || self.rename_colorscheme_flag
            || self.user_wants_to_delete_colorscheme_flag
    }

    /// Take an item off the board and file what became of it.
    ///
    /// Both endings come through here. Completing and deleting used to be
    /// different in kind — completing wrote an archive row, deleting simply
    /// dropped the item, so a task you gave up on left no trace that it had
    /// ever existed and the README's promise that deleted items are kept was
    /// untrue. They are the same act with a different `Outcome` now, and the
    /// ledger is what tells them apart.
    ///
    /// The item is removed from the live set whether or not the write
    /// succeeded: refusing to complete a task because the disk is full is the
    /// wrong trade, so the failure is reported and the removal stands.
    fn retire_active_thing(&mut self, id: u64, outcome: Outcome) {
        // The board files first and only removes if the filing worked: a ✓
        // that reports why it did nothing is a far better failure than one
        // that quietly eats the task. A stale confirmation for an item that
        // is already gone is simply dismissed, not reported.
        let command = match outcome {
            Outcome::Finished => board::Command::Complete { id, at: Some(Local::now()) },
            Outcome::Dropped => board::Command::Delete { id, at: Some(Local::now()) },
        };
        if let Err(error) = self.apply(command)
            && error.status != 410
        {
            self.show_error(error.message);
        }
        if self.planner_selection == Some(id) {
            self.planner_selection = None;
            self.planner_selected_session = None;
        }
        self.dismiss_retire_confirmations_for(id);
    }

    /// Close a complete/delete confirmation that is asking about `id` — and
    /// only that one. The phone retires items too now, and a ✓ tapped there
    /// must not answer a "delete this?" the desk is still looking at about
    /// something else.
    fn dismiss_retire_confirmations_for(&mut self, id: u64) {
        if self.confirm_complete_task == Some(id) {
            self.confirm_complete_task = None;
            self.user_wants_to_complete_task_flag = false;
        }
        if self.confirm_delete_task == Some(id) {
            self.confirm_delete_task = None;
            self.user_wants_to_delete_task_flag = false;
        }
    }

    /// Open or close the archive window.
    ///
    /// Opening reads the log if it has not been read yet, and that is the only
    /// time it is read: closing keeps it, so re-opening is free and the
    /// planner's ghosts can consult the same copy without touching the disk.
    /// The old version threw the loaded rows away on close and re-read the
    /// first page from the file on every open.
    fn toggle_archive(&mut self) {
        self.archive_view.open = !self.archive_view.open;

        if self.archive_view.open {
            self.load_archive();
        } else {
            self.archive_view.selected = None;
            self.archive_view.confirm_forget = None;
        }
    }

    /// Make sure the log is in memory, reporting a read failure rather than
    /// showing an empty archive as though nothing had ever been finished.
    fn load_archive(&mut self) {
        if let Err(error) = self.board.load_archive() {
            self.show_error(format!("Could not read the archive:\n{error}"));
        }
    }

    /// Put an archived item back on the board.
    ///
    /// Only possible because the record keeps the whole item: its sessions, its
    /// estimate and its horizon come back with it, so a restored task lands
    /// exactly where it was rather than as a bare name the priority scorer
    /// treats as corrupt.
    ///
    /// The row leaves the archive. An archive is a record of what is *out of
    /// play*, and a task you are doing again is not out of play — leaving the
    /// row behind would have the ledger claim it was finished while it sat in
    /// the task list.
    /// Returns the id the item is live under — its old one when that was
    /// free, a fresh one otherwise.
    fn restore_archived(&mut self, key: ArchiveKey) -> Option<u64> {
        let restored = self
            .apply_or_report(board::Command::Restore { id: key.id, archived_at: key.archived_at })
            .and_then(|reply| reply.id);
        if self.archive_view.selected == Some(key) {
            self.archive_view.selected = None;
        }
        self.archive_view.confirm_forget = None;
        restored
    }

    /// Delete an archived row for good. The one irreversible act in the app,
    /// which is why it is the only one in the archive that asks first.
    fn forget_archived(&mut self, key: ArchiveKey) {
        self.apply_or_report(board::Command::ForgetArchived { id: key.id, archived_at: key.archived_at });
        if self.archive_view.selected == Some(key) {
            self.archive_view.selected = None;
        }
        self.archive_view.confirm_forget = None;
    }

    /// The archive window.
    ///
    /// The drawing itself is a free function over `(&ArchiveLog, &mut
    /// ArchiveView)` rather than a method, for a plain borrow reason with a
    /// pleasant consequence: the window reads hundreds of rows *and* offers
    /// buttons that mutate the log, which cannot both hold `&mut self`. Taking
    /// the two fields separately lets the rows be borrowed rather than cloned
    /// every frame, and forces the buttons to return an intent instead of
    /// acting mid-draw — so every change to the archive happens here, in one
    /// place, after the frame is laid out.
    fn show_archive(&mut self, ctx: &Context) {
        if !self.archive_view.open {
            return;
        }

        let palette = self.active_colorscheme;
        // The error window answers Escape itself and is drawn over everything;
        // the archive stands down for it rather than closing underneath the
        // message the key was meant to dismiss.
        let owns_keys = !self.error_flag;
        let action = archive_window(ctx, &self.board.archive, &mut self.archive_view, palette, owns_keys);

        match action {
            Some(ArchiveAction::Close) => self.toggle_archive(),
            Some(ArchiveAction::Restore(key)) => {
                self.restore_archived(key);
            }
            Some(ArchiveAction::Forget(key)) => self.forget_archived(key),
            None => {}
        }
    }

    fn display_date_entering(&mut self, ui: &mut Ui) {
        let space_font = FontId::new(14.0, FontFamily::Name("space".into()));

        egui::Frame::default()
            .stroke(Stroke::new(0.9, Color32::from_white_alpha(80)))
            .corner_radius(CornerRadius::same(5))
            .inner_margin(Margin { left: 3, right: 3, top: 0, bottom: 2 })
            .show(ui, |ui| {
                ui.set_max_height(35.0);

                ui.horizontal_centered(|ui| {
                    // Constrain the day picker to the days that actually exist in
                    // the selected month/year (e.g. Feb has 28/29), and snap an
                    // already-selected day back into range when the month changes,
                    // so invalid dates like "Feb 31" can't be entered.
                    let max_day = utilities::days_in_month(self.year_input, self.month_input as u32) as i32;
                    self.day_input = self.day_input.clamp(1, max_day);

                    ComboBox::from_id_salt("day")
                        .width(40.0)
                        .selected_text(RichText::from(format!("{:02}", self.day_input)).font(space_font.clone()))
                        .show_ui(ui, |ui| {
                            for day in 1..=max_day {
                                ui.selectable_value(
                                    &mut self.day_input,
                                    day,
                                    RichText::from(format!("{:02}", day)).font(space_font.clone()),
                                );
                            }
                        });
                    ui.label(RichText::new(".").font(space_font.clone()));

                    ComboBox::from_id_salt("month")
                        .width(40.0)
                        .selected_text(RichText::from(format!("{:02}", self.month_input)).font(space_font.clone()))
                        .show_ui(ui, |ui| {
                            for month in 1..=12 {
                                ui.selectable_value(
                                    &mut self.month_input,
                                    month,
                                    RichText::from(format!("{:02}", month)).font(space_font.clone()),
                                );
                            }
                        });
                    ui.label(RichText::new(".").font(space_font.clone()));

                    ComboBox::from_id_salt("year")
                        .width(60.0)
                        .selected_text(RichText::from(format!("{}", self.year_input)).font(space_font.clone()))
                        .show_ui(ui, |ui| {
                            for year in 2000..=2100 {
                                ui.selectable_value(
                                    &mut self.year_input,
                                    year,
                                    RichText::from(year.to_string()).font(space_font.clone()),
                                );
                            }
                        });

                    ui.add_space(10.0);

                    ComboBox::from_id_salt("hour")
                        .width(35.0)
                        .selected_text(RichText::from(format!("{:02}", self.hour_input)).font(space_font.clone()))
                        .show_ui(ui, |ui| {
                            for hour in 0..=23 {
                                ui.selectable_value(
                                    &mut self.hour_input,
                                    hour,
                                    RichText::from(format!("{:02}", hour)).font(space_font.clone()),
                                );
                            }
                        });
                    ui.label(RichText::new(":").font(space_font.clone()));

                    ComboBox::from_id_salt("minute")
                        .width(35.0)
                        .selected_text(RichText::from(format!("{:02}", self.minute_input)).font(space_font.clone()))
                        .show_ui(ui, |ui| {
                            for minute in 0..=59 {
                                ui.selectable_value(
                                    &mut self.minute_input,
                                    minute,
                                    RichText::from(format!("{:02}", minute)).font(space_font.clone()),
                                );
                            }
                        });
                });
            });
    }

    /// Read the user config, set `key` to a typed value, and write it back.
    /// Single source of truth for the read-parse-set-write boilerplate every
    /// runtime setter used to duplicate. Values go in with their real TOML type
    /// (bool / integer / float-array / string) — never stringified numbers — so
    /// this agrees with the startup writer (`write_normalized_config`).
    fn write_config_value(
        &self,
        key: &str,
        value: impl Into<toml_edit::Value>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        crate::initialization::write_config_value(&self.userconfig_path, key, value)
    }

    /// As `write_config_value`, but routes any failure to the error window
    /// instead of silently dropping it (disk full, permissions, locked file).
    fn persist_config_value(&mut self, key: &str, value: impl Into<toml_edit::Value>) {
        if let Err(e) = self.write_config_value(key, value) {
            self.show_error(format!("Could not save setting \"{}\":\n{}", key, e));
        }
    }

    /// Apply the weeks the settings sheet is offering (`week_input`).
    ///
    /// Called when the number is *settled* — the drag stopped, or the field
    /// lost focus — never per step: each apply rebuilds the calendar model for
    /// up to ten years of days, and doing that on every tick of a drag would
    /// make the control feel like it was fighting back.
    fn set_calendar_weeks(&mut self) {
        let clamped = self.week_input.clamp(
            crate::initialization::CALENDAR_WEEKS_MIN,
            crate::initialization::CALENDAR_WEEKS_MAX,
        );
        self.week_input = clamped;

        if clamped == self.calendar_weeks_to_show {
            return;
        }

        self.calendar_weeks_to_show = clamped;
        self.persist_config_value("calendar_weeks_to_show", clamped as i64);
        // Apply immediately rather than only after a restart: rebuild the
        // calendar model and resize the per-row animation cache.
        self.summarize_calendar();
        self.sync_calendar_caches();
    }
    /// Keep the whole layout inside the window by scaling the UI.
    ///
    /// The three columns are laid out at fixed point sizes adding up to
    /// [`DESIGN_WIDTH_POINTS`], which fits a 1920×1080 monitor at 100% display
    /// scaling — the setup this was built on — and nothing narrower. A HiDPI
    /// display reports *half* as many points as pixels, so a 3024px Retina screen
    /// is 1512 points and silently pushed the weather/notepad column off the right
    /// edge; Windows at 125% or 150% scaling does the same thing.
    ///
    /// Rather than re-tuning the hand-fitted widget geometry, this adjusts egui's
    /// zoom factor, which changes how many points the window is worth. The
    /// automatic value is the largest scale (never above 100%) at which the design
    /// width still fits.
    ///
    /// The computation is a fixed point, not a feedback loop: the window's
    /// physical width and native scale factor are both independent of the zoom, so
    /// `points × zoom` is a constant and re-running this on the next frame yields
    /// the same answer.
    fn apply_ui_scale(&mut self, ctx: &Context) {
        let target = if self.ui_scale_percent == UI_SCALE_AUTO {
            let current_zoom = ctx.zoom_factor();
            let width_in_points = ctx.viewport_rect().width();
            if width_in_points <= 0.0 {
                return;
            }
            // width_in_points * current_zoom is the window's width in
            // native-scale points, which is what the design width is expressed in.
            let fits_at = width_in_points * current_zoom / DESIGN_WIDTH_POINTS;

            // Quantize so the resulting points-per-pixel lands on a quarter
            // step. The app is set in Fixedsys, a pixel font (see
            // `snap_font_points`): an arbitrary scale like 0.78125 gives a
            // points-per-pixel of 1.5625, at which almost no text size is a
            // whole number of pixels and everything looks slightly melted.
            // Rounding *down* to a step keeps the layout fitting — it only ever
            // makes the UI smaller than strictly required.
            let native = ctx
                .input(|i| i.viewport().native_pixels_per_point)
                .unwrap_or(1.0)
                .max(0.1);
            let step = PPP_QUANTUM / native;
            let quantized = (fits_at / step).floor() * step;

            quantized.clamp(UI_SCALE_MIN as f32 / 100.0, UI_SCALE_MAX as f32 / 100.0)
        } else {
            self.ui_scale_percent as f32 / 100.0
        };

        // egui rebuilds every cached galley when the zoom changes, so only write
        // when it actually moved. The epsilon also stops sub-pixel jitter in the
        // automatic value from re-laying out the UI every frame.
        if (ctx.zoom_factor() - target).abs() > 0.001 {
            ctx.set_zoom_factor(target);
        }

        // The text sizes are snapped to whole pixels *at the current scale*, so
        // they have to be recomputed whenever the scale actually changes —
        // otherwise they stay snapped to the scale the app started at.
        let pixels_per_point = ctx.pixels_per_point();
        if (pixels_per_point - self.last_font_ppp).abs() > 0.001 {
            self.last_font_ppp = pixels_per_point;
            set_styles(ctx);
        }
    }

    /// Apply a UI scale: the slider's percentage, or the automatic fit.
    ///
    /// Like the week count, this is called when the control settles rather than
    /// per step — the scale is what every point in the window is measured in,
    /// so changing it mid-drag moves the slider out from under the pointer.
    fn set_ui_scale(&mut self, automatic: bool) {
        self.ui_scale_input = self.ui_scale_input.clamp(UI_SCALE_MIN, UI_SCALE_MAX);
        let applied = if automatic {
            UI_SCALE_AUTO
        } else {
            clamp_ui_scale_percent(self.ui_scale_input)
        };

        if applied == self.ui_scale_percent {
            return;
        }
        self.ui_scale_percent = applied;
        self.persist_config_value("ui_scale_percent", applied as i64);
    }

    /// Persist the background brightness. The value itself is live — it is one
    /// multiply in the draw — so only the write to disk waits for the drag to
    /// stop.
    fn set_background_tint(&mut self) {
        let clamped = self.background_image_tint_percent.clamp(0, 100);
        self.background_image_tint_percent = clamped;
        self.persist_config_value("background_image_tint_percent", clamped as i64);
    }

    /* ─────────────────────── The settings sheet ─────────────────────── */

    /// What the window looks like: the picture behind it, how bright it is, and
    /// which palette tints the items on the calendar.
    fn settings_appearance(&mut self, ui: &mut Ui, ctx: &Context) {
        settings_section(ui, "APPEARANCE", "settings_appearance", |ui| {
            settings_row(ui, "Background", |ui| {
                if self.background_options.is_empty() {
                    settings_note(ui, "nothing in the images folder");
                } else {
                    // An index left over from a picture that has since been
                    // deleted would index off the end of the list — which is
                    // what the old sheet did, and it panicked.
                    self.selected_background_index =
                        self.selected_background_index.min(self.background_options.len() - 1);
                    let previous_index = self.selected_background_index;

                    ComboBox::from_id_salt("background_combo")
                        .width(SETTINGS_CONTROL_WIDTH)
                        .selected_text(
                            RichText::new(&self.background_options[previous_index])
                                .size(SETTINGS_LABEL_SIZE),
                        )
                        .show_ui(ui, |ui| {
                            for (index, name) in self.background_options.iter().enumerate() {
                                ui.selectable_value(
                                    &mut self.selected_background_index,
                                    index,
                                    RichText::new(name).size(SETTINGS_LABEL_SIZE),
                                );
                            }
                        });

                    let reload = settings_button(ui, "Reload")
                        .on_hover_text("Read the picture off disk again");

                    if previous_index != self.selected_background_index || reload.clicked() {
                        let name = self.background_options[self.selected_background_index].clone();
                        self.background_image_texture =
                            Some(set_background(ctx, &self.dirs, name.clone()));
                        self.persist_config_value("background", name);
                    }
                }
            });

            settings_row(ui, "Picture brightness", |ui| {
                let slider = ui.add(
                    egui::Slider::new(&mut self.background_image_tint_percent, 0..=100)
                        .suffix("%")
                        .trailing_fill(true),
                );
                // The picture follows the slider live; only the write to disk
                // waits for the drag to end.
                if slider.drag_stopped() || slider.lost_focus() {
                    self.set_background_tint();
                }
            });

            settings_row(ui, "Colour scheme", |ui| {
                let name = self
                    .colorschemes
                    .get(&self.selected_colorscheme_id)
                    .map(|scheme| scheme.name.clone())
                    .unwrap_or_else(|| "none".to_string());
                ui.label(
                    RichText::new(name)
                        .size(SETTINGS_LABEL_SIZE)
                        .color(Color32::from_white_alpha(205)),
                );
                if settings_button(ui, "Manage").clicked() {
                    self.color_picker_flag = true;
                }
            });
        });
    }

    /// The window itself: how big everything is drawn, where it opens, and what
    /// it shows about itself.
    fn settings_window_section(&mut self, ui: &mut Ui, ctx: &Context) {
        settings_section(ui, "WINDOW", "settings_window", |ui| {
            settings_row(ui, "UI scale", |ui| {
                let mut automatic = self.ui_scale_percent == UI_SCALE_AUTO;
                if ui
                    .checkbox(
                        &mut automatic,
                        RichText::new("Fit to window").size(SETTINGS_LABEL_SIZE),
                    )
                    .changed()
                {
                    self.set_ui_scale(automatic);
                }
                if automatic {
                    settings_note(ui, format!("now {}%", (ctx.zoom_factor() * 100.0).round()));
                }
            });

            settings_row(ui, "", |ui| {
                let automatic = self.ui_scale_percent == UI_SCALE_AUTO;
                let slider = ui.add_enabled(
                    !automatic,
                    egui::Slider::new(&mut self.ui_scale_input, UI_SCALE_MIN..=UI_SCALE_MAX)
                        .suffix("%")
                        .trailing_fill(true),
                );
                // Applied when the drag ends, never during it: the scale is
                // what every point in the window is measured in, so applying it
                // live drags the slider out from under the pointer.
                if slider.drag_stopped() || slider.lost_focus() {
                    self.set_ui_scale(false);
                }
            });

            settings_row(ui, "Opens on", |ui| {
                if self.monitor_options.is_empty() {
                    // winit reports unnamed monitors as `None`, and some
                    // headless or remote setups report none at all.
                    settings_note(ui, "no monitors detected");
                } else {
                    // Sync the index to the saved name, falling back to the
                    // first monitor when the saved one is not plugged in.
                    let previous_index = self
                        .monitor_options
                        .iter()
                        .position(|name| name == &self.selected_monitor_name)
                        .unwrap_or(0);
                    self.selected_monitor_index = previous_index;

                    let selected_text = self
                        .monitor_options
                        .get(previous_index)
                        .cloned()
                        .unwrap_or_default();

                    ComboBox::from_id_salt("monitor_combo")
                        .width(SETTINGS_CONTROL_WIDTH)
                        .selected_text(RichText::new(selected_text).size(SETTINGS_LABEL_SIZE))
                        .show_ui(ui, |ui| {
                            for (index, name) in self.monitor_options.iter().enumerate() {
                                ui.selectable_value(
                                    &mut self.selected_monitor_index,
                                    index,
                                    RichText::new(name).size(SETTINGS_LABEL_SIZE),
                                );
                            }
                        });

                    if previous_index != self.selected_monitor_index {
                        self.set_selected_monitor_name();
                    }
                }
            });

            settings_row(ui, "", |ui| {
                // The window is bound to its monitor at startup, so this is the
                // one setting that cannot take effect where it is made.
                settings_note(ui, "takes effect at the next start");
                if settings_button(ui, "Restart now").clicked() {
                    self.restart_self();
                }
            });

            settings_row(ui, "On startup", |ui| {
                let previous = self.start_in_fullscreen;
                ui.checkbox(
                    &mut self.start_in_fullscreen,
                    RichText::new("Open fullscreen").size(SETTINGS_LABEL_SIZE),
                );
                if previous != self.start_in_fullscreen {
                    self.persist_config_value("start_in_fullscreen", self.start_in_fullscreen);
                }
                settings_note(ui, "F11 either way");
            });

            settings_row(ui, "Diagnostics", |ui| {
                let previous = self.enable_fps_counter;
                ui.checkbox(
                    &mut self.enable_fps_counter,
                    RichText::new("Show the frame rate").size(SETTINGS_LABEL_SIZE),
                );
                if previous != self.enable_fps_counter {
                    self.persist_config_value("enable_fps_counter", self.enable_fps_counter);
                }
            });

            // Off by default, and off is the design (§14.1): on a desktop with a
            // spare monitor the uncapped loop is wanted. On a laptop on battery
            // it is a fan, and this is the one knob for that.
            settings_row(ui, "Frame rate", |ui| {
                let mut capped = self.frame_cap_fps != FRAME_CAP_UNCAPPED;
                if ui
                    .checkbox(&mut capped, RichText::new("Cap it").size(SETTINGS_LABEL_SIZE))
                    .changed()
                {
                    self.set_frame_cap(capped);
                }
                settings_note(
                    ui,
                    if capped { "easier on a laptop battery" } else { "uncapped: as fast as the GPU allows" },
                );
            });

            settings_row(ui, "", |ui| {
                let capped = self.frame_cap_fps != FRAME_CAP_UNCAPPED;
                let slider = ui.add_enabled(
                    capped,
                    egui::Slider::new(&mut self.frame_cap_input, FRAME_CAP_MIN..=FRAME_CAP_MAX)
                        .suffix(" fps")
                        .trailing_fill(true),
                );
                if slider.drag_stopped() || slider.lost_focus() {
                    self.set_frame_cap(true);
                }
            });
        });
    }

    /// Apply the frame-rate setting: the slider's value when `capped`, else
    /// uncapped. Takes effect on the next frame — `App` reads it every time it
    /// schedules one.
    fn set_frame_cap(&mut self, capped: bool) {
        let value = if capped { clamp_frame_cap(self.frame_cap_input.max(FRAME_CAP_MIN)) } else { FRAME_CAP_UNCAPPED };
        if capped {
            self.frame_cap_input = value;
        }
        if value == self.frame_cap_fps {
            return;
        }
        self.frame_cap_fps = value;
        self.persist_config_value("frame_cap_fps", value as i64);
    }

    /// The frame cap while awake, or `FRAME_CAP_UNCAPPED`. For `App`.
    pub fn frame_cap_fps(&self) -> u32 {
        self.frame_cap_fps
    }

    /// One row of the colour-scheme list: the name, and the palette itself as
    /// six swatches.
    ///
    /// The list used to be names alone, which is guesswork: `DUSK` and
    /// `Scheme from "lake.jpg"` are evocative, not informative, and the only
    /// way to find out what either did to the calendar was to select it and
    /// look. The swatches are painted over a dark base because these are
    /// translucent tints meant to sit on a photograph — on nothing at all they
    /// read as nothing.
    fn colorscheme_row(&mut self, ui: &mut Ui, id: u32, name: &str, colors: [[u8; 4]; 6]) {
        let width = ui.available_width().min(SCHEME_LIST_WIDTH);
        let (rect, response) =
            ui.allocate_exact_size(vec2(width, SCHEME_ROW_HEIGHT), egui::Sense::click());

        let selected = self.selected_colorscheme_id == id;
        let painter = ui.painter_at(rect);

        if selected || response.hovered() {
            painter.rect_filled(
                rect,
                CornerRadius::same(6),
                Color32::from_white_alpha(if selected { 34 } else { 14 }),
            );
        }

        let strip = 6.0 * SCHEME_SWATCH + 5.0 * 3.0;

        // The name gets the room the swatches leave and no more, cut with an
        // ellipsis where it doesn't fit. A generated scheme is named after the
        // picture it came from — `Scheme from "pexels-francesco-ungaro-…"` — and
        // painted as a plain string it ran straight under the palette it was
        // supposed to be labelling.
        let name_width = (rect.width() - strip - 26.0).max(40.0);
        let mut job = egui::text::LayoutJob::single_section(
            name.to_string(),
            egui::TextFormat {
                font_id: FontId::new(SETTINGS_LABEL_SIZE, FontFamily::Monospace),
                color: if selected { Color32::WHITE } else { Color32::from_white_alpha(190) },
                ..Default::default()
            },
        );
        job.wrap = egui::text::TextWrapping {
            max_width: name_width,
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('…'),
        };
        let galley = painter.layout_job(job);
        painter.galley(
            pos2(rect.left() + 10.0, rect.center().y - galley.size().y * 0.5),
            galley,
            Color32::WHITE,
        );

        let mut x = rect.right() - 10.0 - strip;
        for color in colors {
            let cell = Rect::from_min_size(
                pos2(x, rect.center().y - SCHEME_SWATCH * 0.5),
                Vec2::splat(SCHEME_SWATCH),
            );
            painter.rect_filled(cell, CornerRadius::same(3), Color32::from_black_alpha(170));
            painter.rect_filled(
                cell,
                CornerRadius::same(3),
                Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]),
            );
            painter.rect_stroke(
                cell,
                CornerRadius::same(3),
                Stroke::new(0.8, Color32::from_white_alpha(45)),
                StrokeKind::Inside,
            );
            x += SCHEME_SWATCH + 3.0;
        }

        if response.clicked() {
            self.selected_colorscheme_id = id;
            self.set_colorscheme();
        }
    }

    /// How much of the calendar there is to scroll through.
    fn settings_calendar(&mut self, ui: &mut Ui) {
        settings_section(ui, "CALENDAR", "settings_calendar", |ui| {
            settings_row(ui, "Weeks shown", |ui| {
                let weeks = ui.add(
                    egui::DragValue::new(&mut self.week_input)
                        .range(
                            crate::initialization::CALENDAR_WEEKS_MIN
                                ..=crate::initialization::CALENDAR_WEEKS_MAX,
                        )
                        .speed(0.5),
                );
                // Applied when the number settles: every apply rebuilds the
                // calendar model for up to ten years of days.
                if weeks.drag_stopped() || weeks.lost_focus() {
                    self.set_calendar_weeks();
                }
                settings_note(
                    ui,
                    format!(
                        "{}–{}",
                        crate::initialization::CALENDAR_WEEKS_MIN,
                        crate::initialization::CALENDAR_WEEKS_MAX
                    ),
                );
            });
        });
    }

    /// Where the forecast is for, and how much of it there is.
    fn settings_weather(&mut self, ui: &mut Ui) {
        settings_section(ui, "WEATHER", "settings_weather", |ui| {
            settings_row(ui, "Location", |ui| {
                ui.add(
                    egui::DragValue::new(&mut self.coordinates[0])
                        .prefix("lat ")
                        .range(-90..=90)
                        .fixed_decimals(2)
                        .speed(0.0025),
                );
                ui.add(
                    egui::DragValue::new(&mut self.coordinates[1])
                        .prefix("lon ")
                        .range(-180..=180)
                        .fixed_decimals(2)
                        .speed(0.005),
                );
            });

            settings_row(ui, "", |ui| {
                if settings_button(ui, "Pick on a map").clicked() {
                    self.coordinates_before_picker = Some(self.coordinates);
                    self.coordinates_map_flag = true;
                }
                // Fetching is a network round trip, so it is asked for rather
                // than fired off after every nudge of a coordinate.
                if settings_button(ui, "Fetch forecast")
                    .on_hover_text("Load the forecast for these coordinates")
                    .clicked()
                {
                    self.set_weather_coordinates();
                }
            });

            settings_row(ui, "Forecast", |ui| {
                let previous = self.three_day_weather;
                ui.checkbox(
                    &mut self.three_day_weather,
                    RichText::new("Three days").size(SETTINGS_LABEL_SIZE),
                );
                if previous != self.three_day_weather {
                    self.persist_config_value("three_day_weather", self.three_day_weather);
                }
                settings_note(ui, "off: that space is the notepad");
            });
        });
    }
    fn set_weather_coordinates(&mut self) {
        let coords = self.coordinates;
        self.weather_service.set_coordinates(coords);
        self.persist_config_value("coordinates", utilities::float_pair_array(coords));
    }
    fn set_selected_monitor_name(&mut self) {
        self.selected_monitor_name = self.monitor_options.get(self.selected_monitor_index).unwrap_or(&"".to_string()).to_string();
        let name: String = self.selected_monitor_name.chars().take(1000).collect();
        self.persist_config_value("selected_monitor_name", name);
    }
    fn fix_and_cache_weather_data(&mut self) {
        self.weather_is_broken_flag = false;
        let static_weather_data = self.weather_service.data.read().map(|w| w.clone())
            .unwrap_or_else(|_| vec![]);

        // The reshape below indexes `static_weather_data[hour][day]` for all 24
        // hours and days 0..=2, so both the outer length (24 hours) and every
        // inner length (>= 3 days) must hold. A partial/short Open-Meteo response
        // (DST edge, API change, truncated payload) would otherwise panic here.
        if static_weather_data.len() != 24
            || static_weather_data.iter().any(|hour| hour.len() < 3)
        {
            self.weather_is_broken_flag = true;
            return ();
        }

        let mut weather_datas = vec![vec![], vec![], vec![]];

        for day in 0..=2 {
            for i in 0..12 {
                let index_first_hour = 2 * i;
                let index_second_hour = index_first_hour + 1;

                let data1 = static_weather_data[index_first_hour][day].clone();
                let data2 = static_weather_data[index_second_hour][day].clone();

                let mut temp_avg = (data1.temp + data2.temp) / 2_f64;

                temp_avg = temp_avg.round();

                let eps = 0.1;
                
                if (temp_avg.abs()) < eps {
                    temp_avg = 0.0;
                }

                //we set the weather code to be the greater one of the two
                let weather_code = data1.weather_code.max(data2.weather_code);

                //we maintain that the icon should contain the sun if the first or second hour is classified as being during the day
                weather_datas[day].push((data1.time, temp_avg, weather_code, data1.is_day == 1 || data2.is_day == 1));
            }
        }

        self.weather_data_cache = weather_datas;
    }
    fn restart_self(&mut self) {
        // Restart by spawning a fresh copy and exiting. Any failure is reported
        // and leaves the current process running rather than panicking — and
        // because we only `exit` on a successful spawn, a failed restart can't
        // tear down the running app or spin a respawn loop.
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                self.show_error(format!("Could not restart (couldn't find the executable):\n{}", e));
                return;
            }
        };

        match Command::new(exe).args(std::env::args().skip(1)).spawn() {
            Ok(_) => {
                self.flush_pending_saves();
                self.shutdown_sync();
                exit(0)
            }
            Err(e) => self.show_error(format!("Could not restart:\n{}", e)),
        }
    }
    fn save_textbox_text(&mut self) {
        if self.should_save_textbox_text {
            // Through the board like every other change: it saves, and tells
            // the phone, which shows the notepad too (§21.5). A silent failure
            // would lose the user's notes; `apply_or_report` surfaces it.
            let text = self.board.notes.clone();
            self.apply_or_report(board::Command::SetNotes { text });
            self.should_save_textbox_text = false;
            self.last_textbox_edit_time = None;
        }
    }

    /// Force-persist any unsaved state before the application exits.
    /// Currently only the notepad text is buffered; this is a no-op when clean.
    pub fn flush_pending_saves(&mut self) {
        self.save_textbox_text();
    }

    /// On the way out: the sync engine files whatever it still holds and
    /// stops, so an edit made in the last second is in `outbox.json` before
    /// the process is gone.
    pub fn shutdown_sync(&mut self) {
        if let Some(sync) = &self.sync {
            sync.shutdown();
        }
    }
    fn set_colorscheme(&mut self) {
        let selected_scheme = if let Some(scheme) = self.colorschemes.get(&self.selected_colorscheme_id) {
            scheme.colors.map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
        } else {
            ColorScheme::default_scheme().colors.map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
        };

        self.active_colorscheme = selected_scheme;

        self.persist_config_value("selected_colorscheme_id", self.selected_colorscheme_id as i64);
        // The phone paints in the selected scheme's colours (§21.4).
        self.phone_repaint();
    }
    fn rename_current_colorscheme(&mut self) {
        self.colorschemes.entry(self.selected_colorscheme_id).or_insert(ColorScheme::default_scheme()).rename(self.colorscheme_rename_input.clone());

        self.add_schemes_2_doc();
    }
    fn duplicate_current_colorscheme(&mut self) {
        let duplicate = self.colorschemes.get(&self.selected_colorscheme_id).unwrap_or(&ColorScheme::default_scheme()).duplicate();

        let new_id = self.colorschemes.keys().max().unwrap_or(&0) + 1;

        self.colorschemes.insert(new_id, duplicate);

        self.add_schemes_2_doc();
    }
    fn currently_selected_colorscheme_is_user_configurable(&self) -> bool {
        self.colorschemes.get(&self.selected_colorscheme_id).unwrap_or(&ColorScheme::default_scheme()).is_user_configurable
    }
    fn delete_current_colorscheme(&mut self) {
        if self.user_wants_to_delete_colorscheme_flag {
            let _ = self.colorschemes.remove_entry(&self.selected_colorscheme_id);

            let new_id = self.colorschemes.keys().max().unwrap_or(&0);
            self.selected_colorscheme_id = *new_id;

            self.set_colorscheme();

            self.add_schemes_2_doc();

            self.user_wants_to_delete_colorscheme_flag = false;
        }
    }
    fn add_schemes_2_doc(&self) {
        let _ = color::save_colorschemes(&self.colorschemes, &self.dirs.data);
    }
    fn save_colorscheme_edits(&mut self) {
        if let Some(scheme) = self.colorscheme_being_edited.take() {
            self.colorschemes.insert(self.selected_colorscheme_id, scheme);
            // The phone paints in these colours (§21.4).
            self.phone_repaint();
        }
    }
    fn try_to_generate_colorscheme(&mut self) {
        // The images folder can be empty — it is, on a fresh install — and
        // the manager is drawn instead of the appearance row that would have
        // said so. Asking for the first of no pictures used to be a crash.
        let Some(name) = self.background_options.get(self.selected_background_index).cloned() else {
            self.show_error(
                "There is no picture to read: put one in the images folder, pick it under Appearance, and try again."
                    .to_string(),
            );
            return;
        };

        match color::generate_colorscheme(&self.dirs, name.clone()) {
            Some(scheme) => {
                let new_id = self.colorschemes.keys().max().unwrap_or(&0) + 1;
                self.colorschemes.insert(new_id, scheme);
                self.add_schemes_2_doc();
            }
            // A button that does nothing looks broken; say what happened.
            None => self.show_error(format!("Could not read {name} as a picture to build a palette from.")),
        }
    }
}

impl TaskApp {
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        // egui 0.35 is Ui-centric: the frame hands us a root `&mut Ui` (via `Context::run_ui`)
        // instead of a `&Context`. Bind an owned clone of the context to a local and reference it,
        // so every existing `ctx.*` / `Window::show(ctx, …)` call below stays unchanged while the
        // top-level panels can still borrow the root `ui` mutably (the clone doesn't borrow `ui`).
        let ctx_owned = ui.ctx().clone();
        let ctx = &ctx_owned;

        // Captured **before** anything is drawn, and for the same reason
        // `naming_owned_frame` is: a window closes itself during its own draw,
        // so by the end of the frame it looks as though nothing was ever open —
        // and the very keypress that closed it is still in this frame's input.
        // Asking afterwards would have `P` close the planner and then reopen it
        // on the same press, forever. Whatever owned the keyboard when the
        // frame began owns it for all of the frame.
        let modal_owned_frame = self.any_modal_open();

        // Fit the fixed-width layout to whatever window we were given. Runs before
        // anything is drawn so the whole frame uses one consistent scale.
        self.apply_ui_scale(ctx);

        if self.background_image_texture.is_none() {
            if let Some(name) = self.pending_initial_background.take() {
                self.background_image_texture = Some(set_background(ctx, &self.dirs, name.clone()));
            }
        }

        if self.enable_fps_counter {
            self.fps_counter.update();
        }
        // F11 is the fullscreen key on Windows and Linux. macOS reserves it for
        // Mission Control's "show desktop" and never delivers it to the app, so
        // accept the platform's own Ctrl+Cmd+F there too.
        let toggle_fullscreen = ctx.input(|i| {
            i.key_pressed(Key::F11)
                || (cfg!(target_os = "macos")
                    && i.modifiers.mac_cmd
                    && i.modifiers.ctrl
                    && i.key_pressed(Key::F))
        });
        if toggle_fullscreen {
            let old_fullscreen = ctx.input(|i| i.viewport().fullscreen).unwrap_or(false);
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(!old_fullscreen));
        }

        let current_weather = self.weather_service.version.load(Ordering::Relaxed);
        if current_weather != self.last_weather_version {
            self.fix_and_cache_weather_data();
            self.last_weather_version = current_weather;
        }

        let old_date = self.date;
        self.date = chrono::Local::now();
        if self.date.day() != old_date.day() {
            self.summarize_calendar();
            self.next_three_weekdays = next_three_weekdays(self.date);
        }

        // Usually already served from `App::user_event`; this catches anything
        // that arrived since, before the frame draws it.
        self.serve_phone_requests();

        // Debounced notepad autosave: persist ~2s after the last edit. This uses
        // wall-clock time so the cadence does not depend on the (uncapped) frame
        // rate. A final flush also runs on exit (App::exiting), so edits made just
        // before quitting are never lost.
        if self.should_save_textbox_text {
            let due = self
                .last_textbox_edit_time
                .map_or(true, |t| t.elapsed() >= std::time::Duration::from_secs(2));
            if due {
                self.save_textbox_text();
            }
        }

        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                if ui.button("New Task").on_hover_text("New task  (T)").clicked() {
                    self.new_task_flag = true;
                    self.dialog_wants_focus = true;
                }
                ui.add_space(12.0);
                if ui.button("New Event").on_hover_text("New event  (E)").clicked() {
                    self.new_event_flag = true;
                    self.dialog_wants_focus = true;
                }
                ui.add_space(12.0);

                // Opens on **the day it was last left**, not on today.
                //
                // Planning a Thursday is not one visit: you open the day, look
                // at the week, come back. Snapping to today each time made
                // every one of those a trip back through the calendar, and the
                // day is remembered anyway — it just wasn't used. Clicking a
                // calendar day still opens *that* day, which is the explicit
                // gesture, and `Today` / `T` are one press away inside.
                if self.planner_flag {
                    if ui.button("Planner").highlight().on_hover_text("Close the planner  (P, Esc)").clicked() {
                        self.close_planner();
                    }
                } else {
                    if ui.button("Planner").on_hover_text("The day you were last on  (P)").clicked() {
                        self.open_planner(self.planner_day);
                    }
                }
                ui.add_space(12.0);

                if self.archive_view.open {
                    if ui.button("Archive").highlight().on_hover_text("Close the archive  (A, Esc)").clicked() {
                        self.toggle_archive();
                    }
                } else {
                    if ui.button("Archive").on_hover_text("What became of everything  (A)").clicked() {
                        self.toggle_archive();
                    }
                }

                ui.add_space(12.0);

                if self.settings_flag {
                    if ui.button("Settings").highlight().on_hover_text("Close settings  (S, Esc)").clicked() {
                        self.settings_flag = false;
                    }
                } else {
                    if ui.button("Settings").on_hover_text("Settings  (S)").clicked() {
                        self.settings_flag = true;
                    }
                }

                // Push a right-aligned layout for the Quit button
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Quit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    self.sync_indicator(ui);

                    ui.add_space(45.0);

                    if self.enable_fps_counter {
                        ui.label(self.fps_counter.fps_text.clone());
                    }
                });
            });
        });

        //PUT THE BACKGROUND COLOR INSIDE THE DEFAULT IN A FRAME
        //THIS IS WHERE YOU CONTROL THE OVERALL PANEL VISUALS
        // `available_rect_before_wrap()` on the root ui — after the menu bar panel above has
        // reserved its space — is the central region: the exact value the removed
        // `ctx.available_rect()` returned (before the CentralPanel's own inner margin).
        let screen_rect = ui.available_rect_before_wrap();
        egui::CentralPanel::default()
        .show(ui, |ui| {

            // Skip the background draw if the texture hasn't been initialized yet
            // rather than unwrapping. In practice it's set from
            // `pending_initial_background` at the top of `ui()`, but guarding here
            // removes the latent crash if that ever fails to run.
            if let Some(background_texture) = self.background_image_texture.as_ref() {
                ctx.layer_painter(egui::LayerId::background()).image(
                    background_texture.id(),
                    screen_rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                    egui::Color32::WHITE.gamma_multiply((self.background_image_tint_percent as f32 / 100.0).clamp(0.0, 1.0)),
                );
            }

            ui.horizontal_top(|ui| {

                ui.add_space(5.0);

                self.show_tasks(ui);

                self.show_calendar(ui);              

                ui.add_space(-20.0);

                self.show_weather_forecast(ui);
            });
        });

        if self.user_wants_to_complete_task_flag {
            if let Some(id) = self.confirm_complete_task {
                // Resolve the cosmetic name for display; if the item is gone
                // (e.g. removed underneath the dialog), dismiss instead of acting.
                if let Some(name) = self.board.items.iter().find(|x| x.id == id).map(|x| x.name.clone()) {
                    egui::Window::new("Confirm Complete")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.label(format!("Are you sure you want to mark \"{}\" as complete?", name));
                            let (accepted, dismissed) = confirmation_keys(ui.ctx());
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if ui.add(Button::new("Yes").min_size(CONFIRM_BUTTON)).clicked() || accepted {
                                    self.retire_active_thing(id, Outcome::Finished);
                                }
                                if ui.add(Button::new("No").min_size(CONFIRM_BUTTON)).clicked() || dismissed {
                                    self.confirm_complete_task = None;
                                    self.user_wants_to_complete_task_flag = false;
                                }
                                confirmation_key_hint(ui);
                            });
                        });
                } else {
                    self.confirm_complete_task = None;
                    self.user_wants_to_complete_task_flag = false;
                }
            }
        }

        if self.user_wants_to_delete_task_flag {
            if let Some(id) = self.confirm_delete_task {
                if let Some(name) = self.board.items.iter().find(|x| x.id == id).map(|x| x.name.clone()) {
                    egui::Window::new("Confirm Delete")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.label(format!("Are you sure you want to delete \"{}\"?", name));
                            // Deleting is no longer the end of the item: it is
                            // filed as dropped, and can be put back from the
                            // archive. Saying so is what makes the button
                            // something you can press without weighing it up.
                            ui.label(
                                RichText::new("It goes to the archive, where it can be restored.")
                                    .size(ARCHIVE_META_SIZE)
                                    .color(Color32::from_white_alpha(150)),
                            );
                            let (accepted, dismissed) = confirmation_keys(ui.ctx());
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if ui.add(Button::new("Yes").min_size(CONFIRM_BUTTON)).clicked() || accepted {
                                    self.retire_active_thing(id, Outcome::Dropped);
                                }
                                if ui.add(Button::new("No").min_size(CONFIRM_BUTTON)).clicked() || dismissed {
                                    self.confirm_delete_task = None;
                                    self.user_wants_to_delete_task_flag = false;
                                }
                                confirmation_key_hint(ui);
                            });
                        });
                } else {
                    self.confirm_delete_task = None;
                    self.user_wants_to_delete_task_flag = false;
                }
            }
        }

        if self.new_event_flag {
            egui::Window::new("Create new event")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_max_size(Vec2::new(300.0, 300.0));
                    ui.vertical(|ui| {
                        ui.add_space(5.0);
                        ui.label("Event Name:");
                        let name = ui.add(egui::TextEdit::singleline(&mut self.event_name_input).hint_text("Attend meeting"));
                        if std::mem::take(&mut self.dialog_wants_focus) {
                            name.request_focus();
                        }

                        ui.add_space(10.0);

                        ui.label("Date:");
                        self.display_date_entering(ui);

                        ui.add_space(15.0);
                        
                        ui.horizontal(|ui| {
                            if ui.button("Ok").clicked() {
                                // Names are cosmetic now (items are keyed by id),
                                // so duplicates are allowed.
                                match utilities::parse_time_input(self.day_input, self.month_input, self.year_input, self.hour_input, self.minute_input) {
                                    Ok(date) => {
                                        self.add_active_thing(self.event_name_input.clone(), Some(date), None, true, None);
                                        self.new_event_flag = false;
                                    },
                                    _ => {
                                        self.show_error("Problem with date".to_string());
                                    },
                                }
                            }

                            if ui.button("Cancel").clicked() {
                                self.new_event_flag = false;
                            }
                        });

                    });

                });
        }

        if self.new_task_flag {
            egui::Window::new("Create new task")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        //insert fields
                        ui.label("Task Name:");
                        let name = ui.add(egui::TextEdit::singleline(&mut self.task_name_input).hint_text("Complete assignment"));
                        // One frame only — see `planner_naming_focus` for what
                        // re-requesting focus every frame does to a field.
                        if std::mem::take(&mut self.dialog_wants_focus) {
                            name.request_focus();
                        }

                        ui.checkbox(&mut self.use_date_for_addable, "Has deadline");

                        // The one knob, and which question it asks depends on
                        // the checkbox above: dated tasks rank by severity
                        // (how bad is missing the date), undated ones by
                        // horizon (roughly how soon it should happen).
                        if self.use_date_for_addable {
                            ui.label("Severity:");
                            ComboBox::from_id_salt("importance combo")
                                .selected_text(IMPORTANCE[(self.task_importance_input as usize).min(IMPORTANCE.len() - 1)])
                                .show_ui(ui, |ui| {
                                    for (i, importance) in IMPORTANCE.iter().enumerate() {
                                        ui.selectable_value(
                                            &mut self.task_importance_input,
                                            i as u8,
                                            importance.to_string(),
                                        );
                                    }
                                });

                            ui.label("Date:");
                            self.display_date_entering(ui);
                        } else {
                            ui.label("Horizon:");
                            ComboBox::from_id_salt("urgency combo")
                                .selected_text(HORIZON[(self.time_importance_input as usize).min(HORIZON.len() - 1)])
                                .show_ui(ui, |ui| {
                                    for i in HORIZON_DISPLAY_ORDER {
                                        ui.selectable_value(
                                            &mut self.time_importance_input,
                                            i,
                                            HORIZON[i as usize].to_string(),
                                        );
                                    }
                                });
                        }
                        
                        ui.add_space(7.0);
                        
                        ui.horizontal(|ui| {
                            if ui.button("Ok").clicked() {
                                let importance = self.task_importance_input;
                                let date = utilities::parse_time_input(self.day_input, self.month_input, self.year_input, self.hour_input, self.minute_input);

                                // Names are cosmetic now (items are keyed by id),
                                // so duplicates are allowed.
                                if !self.use_date_for_addable {
                                    self.add_active_thing(self.task_name_input.clone(), None, None, false, Some(self.time_importance_input));
                                    self.new_task_flag = false;
                                } else {
                                    match date {
                                        Ok(date) => {
                                            self.add_active_thing(self.task_name_input.clone(), Some(date), Some(importance), false, None);
                                            self.new_task_flag = false;
                                        },
                                        _ => {self.show_error("Problem with date".to_string())},
                                    }
                                }
                            }

                            if ui.button("Cancel").clicked() {
                                self.new_task_flag = false;
                            }
                        });
                    });

                });
        }
        
        // The one per-day view. A separate popup used to be drawn after this,
        // listing the day the planner is already showing and offering a "Plan
        // day" button to hand over to it; clicking a calendar day now comes
        // straight here.
        self.show_planner(ctx);

        self.show_archive(ctx);

        if self.settings_flag && !self.color_picker_flag {
            egui::Window::new("Settings")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .default_width(SETTINGS_WIDTH)
                .show(ctx, |ui| {
                    ui.set_width(SETTINGS_WIDTH);
                    // Sliders are the widest control on the sheet and egui's
                    // default is a stub; set it once for all of them.
                    ui.spacing_mut().slider_width = SETTINGS_CONTROL_WIDTH;

                    // The sheet scrolls rather than being cut off. It used to be
                    // pinned to a fixed 400×300 with far more than that inside
                    // it, so the bottom rows simply fell out of the window.
                    egui::ScrollArea::vertical()
                        .max_height(SETTINGS_MAX_BODY_HEIGHT)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            self.settings_appearance(ui, ctx);
                            self.settings_window_section(ui, ctx);
                            self.settings_calendar(ui);
                            self.settings_weather(ui);
                            self.settings_calendars(ui);
                            self.settings_phone(ui, ctx);
                            self.settings_server(ui);
                            ui.add_space(4.0);
                        });

                    ui.add_space(8.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("TaskDeck  ·  Timi Salonen")
                                .size(SETTINGS_FINE_SIZE)
                                .color(Color32::from_white_alpha(110)),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(
                                    Button::new(RichText::new("Done").size(SETTINGS_LABEL_SIZE))
                                        .min_size(vec2(92.0, 32.0)),
                                )
                                .clicked()
                            {
                                self.settings_flag = false;
                            }
                        });
                    });
                });

            // Escape closes the sheet, the way it closes the planner. Not while
            // the map picker is stacked over it (Escape belongs to the topmost
            // window), and not while a field has the keyboard.
            if !self.coordinates_map_flag
                && !ctx.egui_wants_keyboard_input()
                && ctx.input(|i| i.key_pressed(Key::Escape))
            {
                self.settings_flag = false;
            }
        }

        if self.coordinates_map_flag {
            egui::Window::new("Pick a location")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    // Sized from the viewport, and the window then sizes itself
                    // to *this*. The old picker did the opposite: a window
                    // pinned to a fixed 1500×770 holding a 1440×712 map plus a
                    // footer laid out at absolute rects derived from
                    // `available_rect_before_wrap()`. The content came to a few
                    // points more than the window, so every frame the window
                    // tried to grow, `fixed_size` pulled it back, the available
                    // rect moved, and the map was drawn somewhere slightly
                    // different — which is what the shaking was.
                    let viewport = ctx.viewport_rect();
                    // The floor is what the footer needs — a coordinate, the
                    // nearest city, the pointer's own reading, and the buttons —
                    // rather than what the map could get away with.
                    let width = (viewport.width() - MAP_WINDOW_INSET)
                        .clamp(MAP_MIN_WIDTH, MAP_MAX_WIDTH)
                        .min((viewport.height() - MAP_WINDOW_INSET) * 2.0);
                    // The picture is equirectangular and 2:1; anything else
                    // stretches the world.
                    let map_size = vec2(width, width * 0.5);

                    let (rect, response) =
                        ui.allocate_exact_size(map_size, egui::Sense::click_and_drag());
                    let painter = ui.painter_at(rect);

                    // ── zoom, about the pointer ──────────────────────────
                    if response.hovered() {
                        let scroll = ui.input(|i| i.smooth_scroll_delta.y) * 2.0;
                        if scroll != 0.0 {
                            let old_zoom = self.map_zoom;
                            self.map_zoom = (self.map_zoom * (scroll * 0.001).exp()).clamp(1.0, 15.0);

                            if let Some(cursor) = response.hover_pos() {
                                let cursor_uv = egui::pos2(
                                    (cursor.x - rect.min.x) / rect.width(),
                                    (cursor.y - rect.min.y) / rect.height(),
                                );
                                let old_uv = Vec2::splat(1.0 / old_zoom);
                                let new_uv = Vec2::splat(1.0 / self.map_zoom);
                                self.map_offset += cursor_uv.to_vec2() * (old_uv - new_uv);
                            }
                        }
                    }

                    // ── pan ──────────────────────────────────────────────
                    // Either button. Left-dragging a map moves the map
                    // everywhere else; egui tells a drag from a click, so this
                    // costs the click-to-pick nothing.
                    if response.dragged() {
                        let delta = response.drag_delta();
                        self.map_offset.x -= delta.x / rect.width() / self.map_zoom;
                        self.map_offset.y -= delta.y / rect.height() / self.map_zoom;
                    }

                    let uv_size = Vec2::splat(1.0 / self.map_zoom);
                    self.map_offset.x = self.map_offset.x.clamp(0.0, 1.0 - uv_size.x);
                    self.map_offset.y = self.map_offset.y.clamp(0.0, 1.0 - uv_size.y);
                    let uv_min = pos2(self.map_offset.x, self.map_offset.y);
                    let uv_max = pos2(uv_min.x + uv_size.x, uv_min.y + uv_size.y);

                    // Where a point on the globe lands on screen, and back.
                    let to_screen = |lat: f32, lon: f32| -> Pos2 {
                        let u = ((lon + 180.0) / 360.0 - uv_min.x) / uv_size.x;
                        let v = ((90.0 - lat) / 180.0 - uv_min.y) / uv_size.y;
                        pos2(rect.left() + u * rect.width(), rect.top() + v * rect.height())
                    };
                    let to_globe = |at: Pos2| -> (f32, f32) {
                        let u = uv_min.x + (at.x - rect.left()) / rect.width() * uv_size.x;
                        let v = uv_min.y + (at.y - rect.top()) / rect.height() * uv_size.y;
                        (90.0 - v * 180.0, u * 360.0 - 180.0)
                    };

                    // ── the world ────────────────────────────────────────
                    if let Some(texture) = &self.map_texture {
                        painter.image(
                            texture.id(),
                            rect,
                            Rect::from_min_max(uv_min, uv_max),
                            Color32::WHITE,
                        );
                    }

                    // Graticule: every thirty degrees, with the equator and the
                    // prime meridian a shade brighter. It is what makes a
                    // picture of the Earth read as a map you can point at.
                    for lon in (-180..=180).step_by(30) {
                        let lon = lon as f32;
                        let top = to_screen(90.0, lon);
                        let bottom = to_screen(-90.0, lon);
                        painter.line_segment(
                            [pos2(top.x, rect.top()), pos2(bottom.x, rect.bottom())],
                            Stroke::new(0.6, Color32::from_white_alpha(if lon == 0.0 { 60 } else { 26 })),
                        );
                    }
                    for lat in (-90..=90).step_by(30) {
                        let lat = lat as f32;
                        let left = to_screen(lat, -180.0);
                        painter.line_segment(
                            [pos2(rect.left(), left.y), pos2(rect.right(), left.y)],
                            Stroke::new(0.6, Color32::from_white_alpha(if lat == 0.0 { 60 } else { 26 })),
                        );
                    }

                    // ── pick ─────────────────────────────────────────────
                    if response.clicked() {
                        if let Some(at) = response.interact_pointer_pos() {
                            let (lat, lon) = to_globe(at);
                            self.coordinates = [lat, lon];
                        }
                    }

                    // ── the chosen point ─────────────────────────────────
                    // A crosshair across the whole map rather than a dot: at
                    // this scale a dot is a pixel of noise over a photograph of
                    // a planet, and the lines say which latitude and which
                    // longitude, which is what was actually picked.
                    let marker = to_screen(self.coordinates[0], self.coordinates[1]);
                    let accent = Color32::from_rgb(255, 120, 90);
                    if rect.contains(marker) {
                        painter.line_segment(
                            [pos2(rect.left(), marker.y), pos2(rect.right(), marker.y)],
                            Stroke::new(0.8, accent.gamma_multiply(0.55)),
                        );
                        painter.line_segment(
                            [pos2(marker.x, rect.top()), pos2(marker.x, rect.bottom())],
                            Stroke::new(0.8, accent.gamma_multiply(0.55)),
                        );
                        painter.circle_stroke(marker, 7.0, Stroke::new(1.6, accent));
                        painter.circle_filled(marker, 2.5, accent);
                    }

                    // ── cities ───────────────────────────────────────────
                    // Two hundred of them, so they are dim until asked about:
                    // a dot to say a place is there, its name only under the
                    // pointer.
                    let hover = response.hover_pos();
                    for city in weather::CITIES {
                        let at = to_screen(city.latitude, city.longitude);
                        if !rect.contains(at) {
                            continue;
                        }
                        let near = hover.is_some_and(|pointer| at.distance(pointer) < 9.0);
                        painter.circle_filled(
                            at,
                            if near { 4.0 } else { 2.0 },
                            if near { Color32::WHITE } else { Color32::from_white_alpha(120) },
                        );
                        if near {
                            let label = painter.layout_no_wrap(
                                city.name.to_string(),
                                FontId::new(SETTINGS_FINE_SIZE, FontFamily::Monospace),
                                Color32::WHITE,
                            );
                            let box_rect = Rect::from_min_size(
                                at + vec2(8.0, -label.size().y - 8.0),
                                label.size() + vec2(10.0, 6.0),
                            );
                            painter.rect_filled(box_rect, CornerRadius::same(4), Color32::from_black_alpha(190));
                            painter.galley(box_rect.min + vec2(5.0, 3.0), label, Color32::WHITE);
                        }
                    }

                    painter.rect_stroke(
                        rect,
                        CornerRadius::same(6),
                        Stroke::new(1.0, Color32::from_white_alpha(70)),
                        StrokeKind::Inside,
                    );

                    // ── footer ───────────────────────────────────────────
                    ui.add_space(8.0);
                    // The gestures go on their own line: in the row below they
                    // would be drawn under the buttons on a narrow window, that
                    // row being laid out from both ends at once.
                    settings_note(ui, "scroll to zoom  ·  drag to pan  ·  click to pick");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "{:.2}, {:.2}",
                                self.coordinates[0], self.coordinates[1]
                            ))
                            .size(SETTINGS_LABEL_SIZE),
                        );

                        // The nearest of the two hundred marked cities — a
                        // sanity check in words for a point picked by eye.
                        if let Some(city) = weather::nearest_city(self.coordinates[0], self.coordinates[1]) {
                            settings_note(ui, format!("near {}", city.name));
                        }

                        // What the pointer is over, so a coordinate can be read
                        // off the map without committing to it.
                        if let Some(pointer) = hover.filter(|at| rect.contains(*at)) {
                            let (lat, lon) = to_globe(pointer);
                            settings_note(ui, format!("·  pointer {lat:.2}, {lon:.2}"));
                        }

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(
                                    Button::new(RichText::new("Use this").size(SETTINGS_LABEL_SIZE))
                                        .min_size(vec2(92.0, 32.0)),
                                )
                                .clicked()
                            {
                                // Picking a place on a map *is* choosing it —
                                // it used to leave the coordinates staged and
                                // the forecast still on the old ones until you
                                // found the Apply button behind this window.
                                self.set_weather_coordinates();
                                self.coordinates_map_flag = false;
                            }
                            if ui
                                .add(
                                    Button::new(RichText::new("Cancel").size(SETTINGS_LABEL_SIZE))
                                        .min_size(vec2(92.0, 32.0)),
                                )
                                .clicked()
                            {
                                if let Some(previous) = self.coordinates_before_picker {
                                    self.coordinates = previous;
                                }
                                self.coordinates_map_flag = false;
                            }
                            if self.map_zoom > 1.0
                                && ui
                                    .add(
                                        Button::new(RichText::new("Whole world").size(SETTINGS_LABEL_SIZE))
                                            .min_size(vec2(0.0, 32.0)),
                                    )
                                    .clicked()
                            {
                                self.map_zoom = 1.0;
                                self.map_offset = Vec2::ZERO;
                            }
                        });
                    });
                });
        }

        if self.color_picker_flag && !self.edit_colorscheme_flag {
            egui::Window::new("Colour schemes")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .default_width(SETTINGS_WIDTH)
                .show(ctx, |ui| {
                    ui.set_width(SETTINGS_WIDTH);

                    // Sorted, and taken as a list rather than iterated straight
                    // off the HashMap: map order is arbitrary *and differs
                    // between runs*, so the schemes reshuffled themselves at
                    // every launch. By id, the built-ins keep the order they
                    // are defined in and a new scheme lands at the bottom,
                    // where the user just made it.
                    let mut listed: Vec<(u32, String, [[u8; 4]; 6], bool)> = self
                        .colorschemes
                        .iter()
                        .map(|(id, scheme)| {
                            (*id, scheme.name.clone(), scheme.colors, scheme.is_builtin())
                        })
                        .collect();
                    listed.sort_unstable_by_key(|(id, ..)| *id);
                    let (builtin, mine): (Vec<_>, Vec<_>) =
                        listed.into_iter().partition(|(.., builtin)| *builtin);

                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(SCHEME_LIST_WIDTH);

                            // Both sections are labelled. They used to be an
                            // unlabelled pair split on a flag nothing ever set,
                            // so every scheme landed in the first list and the
                            // second was permanently empty.
                            settings_section_heading(ui, "BUILT IN");
                            // Tall enough for the built-ins there actually are:
                            // a constant here meant that adding one hid it
                            // below the fold of a list nobody expects to
                            // scroll.
                            egui::ScrollArea::vertical()
                                .scroll_source(egui::scroll_area::ScrollSource::ALL)
                                .max_height(SCHEME_ROW_PITCH * (builtin.len() as f32).min(9.0))
                                .id_salt("builtin_schemes")
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = SCHEME_ROW_GAP;
                                    for (id, name, colors, _) in &builtin {
                                        self.colorscheme_row(ui, *id, name, *colors);
                                    }
                                });

                            ui.add_space(10.0);
                            settings_section_heading(ui, "YOURS");
                            if mine.is_empty() {
                                settings_note(ui, "duplicate one to get a scheme you can edit");
                            }
                            egui::ScrollArea::vertical()
                                .scroll_source(egui::scroll_area::ScrollSource::ALL)
                                .max_height(SCHEME_ROW_PITCH * (mine.len() as f32).clamp(1.0, 8.0))
                                .id_salt("user_schemes")
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = SCHEME_ROW_GAP;
                                    for (id, name, colors, _) in &mine {
                                        self.colorscheme_row(ui, *id, name, *colors);
                                    }
                                });
                        });

                        ui.add_space(18.0);

                        ui.vertical(|ui| {
                            let mine = self.currently_selected_colorscheme_is_user_configurable();
                            let wide = vec2(230.0, 30.0);

                            if ui
                                .add(Button::new(RichText::new("Duplicate").size(SETTINGS_LABEL_SIZE)).min_size(wide))
                                .on_hover_text("Make an editable copy of the selected scheme")
                                .clicked()
                            {
                                self.duplicate_current_colorscheme();
                            }

                            // The three verbs a built-in doesn't answer to are
                            // shown disabled rather than hidden: a button column
                            // that grows and shrinks as the selection moves is
                            // harder to aim at than one that greys out.
                            let edit = ui.add_enabled(
                                mine,
                                Button::new(RichText::new("Edit").size(SETTINGS_LABEL_SIZE)).min_size(wide),
                            );
                            let rename = ui.add_enabled(
                                mine,
                                Button::new(RichText::new("Rename").size(SETTINGS_LABEL_SIZE)).min_size(wide),
                            );
                            let delete = ui.add_enabled(
                                mine,
                                Button::new(RichText::new("Delete").size(SETTINGS_LABEL_SIZE)).min_size(wide),
                            );

                            if !mine {
                                settings_note(ui, "built-in schemes stay as they are");
                            }

                            if edit.clicked() {
                                self.edit_colorscheme_flag = true;
                                self.colorscheme_being_edited = Some(
                                    self.colorschemes
                                        .get(&self.selected_colorscheme_id)
                                        .unwrap_or(&ColorScheme::default_scheme())
                                        .clone(),
                                );
                            }
                            if rename.clicked() {
                                self.rename_colorscheme_flag = true;
                                // Seeded with the current name, so renaming is
                                // an edit rather than a retype.
                                self.colorscheme_rename_input = self
                                    .colorschemes
                                    .get(&self.selected_colorscheme_id)
                                    .map(|scheme| scheme.name.clone())
                                    .unwrap_or_default();
                            }
                            if delete.clicked() {
                                self.user_wants_to_delete_colorscheme_flag = true;
                            }

                            ui.add_space(10.0);
                            ui.separator();
                            ui.add_space(10.0);

                            if ui
                                .add_enabled(
                                    !self.background_options.is_empty(),
                                    Button::new(
                                        RichText::new("Generate from the background")
                                            .size(SETTINGS_LABEL_SIZE),
                                    )
                                    .min_size(wide),
                                )
                                .on_hover_text("Read the background picture and build a palette out of it")
                                .on_disabled_hover_text("Nothing in the images folder to read")
                                .clicked()
                            {
                                self.try_to_generate_colorscheme();
                            }
                        });
                    });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        settings_note(ui, "the ramp runs least to most urgent; the last swatch is events");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(
                                    Button::new(RichText::new("Done").size(SETTINGS_LABEL_SIZE))
                                        .min_size(vec2(92.0, 32.0)),
                                )
                                .clicked()
                            {
                                self.color_picker_flag = false;
                            }
                        });
                    });
                });
        }

        if self.edit_colorscheme_flag && !self.rename_colorscheme_flag && !self.user_wants_to_delete_colorscheme_flag {
            let mut should_save = false;
            let mut should_cancel = false;

            if let Some(scheme) = &mut self.colorscheme_being_edited {
                egui::Window::new("Edit scheme")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .default_width(SCHEME_EDITOR_WIDTH)
                    .show(ctx, |ui| {
                        ui.set_width(SCHEME_EDITOR_WIDTH);

                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(&scheme.name)
                                .size(SETTINGS_LABEL_SIZE)
                                .color(Color32::from_white_alpha(215)),
                        );
                        ui.add_space(12.0);

                        // The six swatches are not six of the same thing, and
                        // the window now says so: five are a ramp with a
                        // direction, the sixth is a different question
                        // altogether. Before, they were one anonymous row of
                        // squares and the only way to find out which was which
                        // was to change one and go and look at the calendar.
                        let mut swap_with: Option<usize> = None;

                        settings_section_heading(ui, "URGENCY");
                        ui.horizontal(|ui| {
                            for index in 0..5 {
                                colorscheme_swatch(
                                    ui,
                                    &mut scheme.colors[index],
                                    index,
                                    &mut self.dragged_color_index,
                                    &mut swap_with,
                                );
                                ui.add_space(8.0);
                            }
                            settings_note(ui, "least → most");
                        });

                        ui.add_space(14.0);
                        settings_section_heading(ui, "EVENTS");
                        ui.horizontal(|ui| {
                            colorscheme_swatch(
                                ui,
                                &mut scheme.colors[5],
                                5,
                                &mut self.dragged_color_index,
                                &mut swap_with,
                            );
                            ui.add_space(8.0);
                            settings_note(ui, "something that happens at a time");
                        });

                        // The swap lands after the row is drawn, so no swatch is
                        // painted from a palette that changed under it.
                        if let (Some(from), Some(to)) = (self.dragged_color_index, swap_with) {
                            scheme.colors.swap(from, to);
                            self.dragged_color_index = Some(to);
                        }
                        if ui.input(|i| i.pointer.any_released()) {
                            self.dragged_color_index = None;
                        }

                        ui.add_space(16.0);
                        settings_section_heading(ui, "ON THE CALENDAR");

                        // What the palette will actually look like: over a dark
                        // ground, as the items themselves are drawn. The
                        // swatches above are the honest editing view — colour
                        // over a checkerboard, so alpha is visible as alpha —
                        // which is exactly what you cannot judge the result
                        // from, these being translucent tints meant for a
                        // photograph.
                        let (strip, _) = ui.allocate_exact_size(
                            vec2(SCHEME_EDITOR_WIDTH, SCHEME_PREVIEW_HEIGHT),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter_at(strip);
                        painter.rect_filled(strip, CornerRadius::same(8), Color32::from_black_alpha(160));
                        let pill_width = (strip.width() - 7.0 * 6.0) / 6.0;
                        for (index, color) in scheme.colors.iter().enumerate() {
                            let left = strip.left() + 6.0 + index as f32 * (pill_width + 6.0);
                            let pill = Rect::from_min_size(
                                pos2(left, strip.top() + 6.0),
                                vec2(pill_width, strip.height() - 12.0),
                            );
                            painter.rect_filled(
                                pill,
                                CornerRadius::same(6),
                                Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]),
                            );
                            painter.rect_stroke(
                                pill,
                                CornerRadius::same(6),
                                Stroke::new(0.8, Color32::from_white_alpha(40)),
                                StrokeKind::Inside,
                            );
                        }

                        ui.add_space(14.0);
                        // On its own line: in the button row it was drawn
                        // underneath them, the row being laid out from both ends
                        // at once.
                        settings_note(ui, "click a swatch to change it, drag one onto another to swap");
                        ui.add_space(8.0);
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(
                                        Button::new(RichText::new("Save").size(SETTINGS_LABEL_SIZE))
                                            .min_size(vec2(92.0, 32.0)),
                                    )
                                    .clicked()
                                {
                                    should_save = true;
                                }
                                if ui
                                    .add(
                                        Button::new(RichText::new("Cancel").size(SETTINGS_LABEL_SIZE))
                                            .min_size(vec2(92.0, 32.0)),
                                    )
                                    .clicked()
                                {
                                    should_cancel = true;
                                }
                            });
                        });
                    });

                    // The edit is live on the calendar behind the window, which
                    // is the only way to judge a palette meant for it.
                    self.active_colorscheme = scheme.colors.map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]));
            }

            if should_save {
                self.save_colorscheme_edits();
                self.set_colorscheme();
                self.edit_colorscheme_flag = false;
            }
            if should_cancel {
                self.set_colorscheme();
                self.edit_colorscheme_flag = false;
            }
        }

        if self.rename_colorscheme_flag && !self.user_wants_to_delete_colorscheme_flag && self.currently_selected_colorscheme_is_user_configurable() {
            egui::Window::new("Rename selected colorscheme")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .min_size(Vec2::new(400.0, 400.0))
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {

                        ui.text_edit_singleline(&mut self.colorscheme_rename_input);

                        ui.add_space(5.0);

                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            let button = ui.add(Button::new("Cancel").min_size(Vec2::new(50.0, 30.0)));
                            if button.clicked() {
                                self.rename_colorscheme_flag = false;
                            }

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.add_space(10.0);
                                let button = ui.add(Button::new("Save").min_size(Vec2::new(50.0, 30.0)));
                                if button.clicked() {
                                    self.rename_current_colorscheme();
                                    self.rename_colorscheme_flag = false;
                                }
                            });
                        });
                    });
                });
        }

        if self.user_wants_to_delete_colorscheme_flag && self.currently_selected_colorscheme_is_user_configurable() && !self.rename_colorscheme_flag {
            egui::Window::new("Confirm Delete Scheme")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!("Are you sure you want to delete \"{}\"?", self.colorschemes.get(&self.selected_colorscheme_id).unwrap_or(&ColorScheme::default_scheme()).name));
                    ui.horizontal(|ui| {
                        if ui.button("Yes").clicked() {
                            self.delete_current_colorscheme();
                        }
                        if ui.button("No").clicked() {
                            self.user_wants_to_delete_colorscheme_flag = false;
                        }
                    });
                });
        }

        //this should be displayed last such that the error window is always on top
        if self.error_flag {
            egui::Window::new("error window")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .min_size(Vec2::new(400.0, 400.0))
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(5.0);
                        // Scrolls rather than grows: errors accumulate here
                        // now (`show_error`), and a window taller than the
                        // screen has its Ok button somewhere off it.
                        egui::ScrollArea::vertical()
                            .max_height(ERROR_TEXT_MAX_HEIGHT)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.colored_label(Color32::from_white_alpha(180), &self.error_text);
                            });

                        ui.add_space(15.0);

                        let button = ui.add(Button::new("Ok").min_size(Vec2::new(50.0, 30.0)));

                        // Either key dismisses: there is only one thing to do
                        // with an error you have read.
                        let (accepted, dismissed) = confirmation_keys(ui.ctx());
                        if button.clicked() || accepted || dismissed {
                            self.error_flag = false;
                        }
                    });
                });
        }

    // Last, after every window has had its frame. Each one claims the keys it
    // owns while it is drawn (the planner's own handler, the archive's, the
    // confirmations'); these two pick up what is left — the menu bar's letters
    // when nothing is open, and Escape for the two windows that answer nothing
    // of their own.
    self.handle_overlay_close_keys(ctx);
    self.handle_menu_keys(ctx, modal_owned_frame);

    if self.any_modal_open() {
        self.hovered_calendar_cell = None;
    }
    }
}

/// The pixel grid Fixedsys Excelsior is drawn on. Its outlines trace a bitmap
/// font's pixels, so a glyph is sharp when its em box lands on a whole number of
/// pixels — and sharpest at a multiple of this — and smeared across two pixels
/// when it doesn't. Rendering the same string at 16.00px and 17.19px side by
/// side makes the difference obvious.
const FIXEDSYS_GRID_PX: f32 = 16.0;
/// How far a requested size may be nudged to land on the grid. Beyond this the
/// size change would be more noticeable than the blur it fixes, so the size
/// settles for the nearest whole pixel instead.
const FIXEDSYS_GRID_TOLERANCE: f32 = 0.09;

/// Adjust a point size so Fixedsys renders on pixel boundaries at the current
/// scale.
///
/// Returns a *point* size, because that is what egui takes; the value is chosen
/// so `size × pixels_per_point` is a whole number of pixels, and a multiple of
/// [`FIXEDSYS_GRID_PX`] when one is close enough to be worth taking.
fn snap_font_points(points: f32, pixels_per_point: f32) -> f32 {
    if pixels_per_point <= 0.0 || points <= 0.0 {
        return points;
    }

    let requested_px = points * pixels_per_point;
    let nearest_grid_px = (requested_px / FIXEDSYS_GRID_PX).round().max(1.0) * FIXEDSYS_GRID_PX;

    let snapped_px = if (nearest_grid_px - requested_px).abs() / requested_px <= FIXEDSYS_GRID_TOLERANCE {
        nearest_grid_px
    } else {
        requested_px.round().max(1.0)
    };

    snapped_px / pixels_per_point
}

pub fn set_styles(ctx: &egui::Context) {
    let pixels_per_point = ctx.pixels_per_point();
    let font = |points: f32| {
        egui::FontId::new(snap_font_points(points, pixels_per_point), egui::FontFamily::Monospace)
    };

    let mut style = (*ctx.global_style()).clone();
    style.text_styles = [
        (egui::TextStyle::Heading, font(30.0)),
        (egui::TextStyle::Body, font(18.0)),
        (egui::TextStyle::Button, font(22.0)),
        (egui::TextStyle::Small, font(11.0)),
        (egui::TextStyle::Monospace, font(11.0)),
    ]
    .into();
    ctx.set_global_style(style);
}

pub fn load_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "fixedsys".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(r#"../fonts/FSEX300.ttf"#))),
    );
    fonts.font_data.insert(
        "dejavu".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(r#"../fonts/DejaVuSans.ttf"#))),
    );
        fonts.font_data.insert(
        "anton".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(r#"../fonts/Anton-Regular.ttf"#))),
    );
        fonts.font_data.insert(
        "space".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(r#"../fonts/SpaceMono-Regular.ttf"#))),
    );
        fonts.font_data.insert(
        "spaceb".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(r#"../fonts/LexendGiga-Light.ttf"#))),
    );
        fonts.font_data.insert(
        "bungee".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(r#"../fonts/FacultyGlyphic-Regular.ttf"#))),
    );    

    fonts.families.get_mut(&egui::FontFamily::Monospace).unwrap().clear();

    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .push("fixedsys".to_owned());
    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .push("dejavu".to_owned());
    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .push("space".to_owned());
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .push("spaceb".to_owned());

    fonts.families.insert(FontFamily::Name("anton".into()), vec!["anton".to_owned()]);

    fonts.families.insert(FontFamily::Name("dejavu".into()), vec!["dejavu".to_owned()]);

    fonts.families.insert(FontFamily::Name("space".into()), vec!["space".to_owned()]);

    fonts.families.insert(FontFamily::Name("spaceb".into()), vec!["spaceb".to_owned()]);

    fonts.families.insert(FontFamily::Name("bungee".into()), vec!["bungee".to_owned()]);

    ctx.set_fonts(fonts);
}

fn attempt_background(path: PathBuf) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>, Box<dyn Error>> {
    let image_bytes = fs::read(&path)?;
    let image = image::load_from_memory(&image_bytes)?
        .to_rgba8();

    Ok(image)
}

fn set_background(ctx: &Context, dirs: &AppDirs, name: String) -> TextureHandle {
    // Fall back to the bundled placeholder if the name is unusable or the file
    // can't be loaded. `image_path` keeps this confined to `images/`.
    let image = dirs.image_path(&name)
        .and_then(|path| attempt_background(path).ok())
        .unwrap_or_else(|| {
            image::load_from_memory(include_bytes!("../noback.png"))
                .expect("Did not get access to fallback background")
                .to_rgba8()
        });

    // `Context::load_texture` *panics* on an image wider or taller than the GPU's
    // limit, so an oversized picture dropped into `images/` would take the whole
    // app down. Shrink it to fit instead — this is a full-window backdrop, so
    // anything past the limit is detail nobody can see anyway. The limit comes
    // from the adapter (commonly 8192 or 16384), so this only bites on genuinely
    // huge photographs.
    let max_side = ctx.input(|i| i.max_texture_side) as u32;
    let longest_side = image.width().max(image.height());
    let image = if longest_side > max_side {
        // Scale both axes by the same factor so the picture isn't stretched.
        let scale = max_side as f32 / longest_side as f32;
        image::imageops::resize(
            &image,
            ((image.width() as f32 * scale) as u32).max(1),
            ((image.height() as f32 * scale) as u32).max(1),
            image::imageops::FilterType::Triangle,
        )
    } else {
        image
    };

    let size = [image.width() as usize, image.height() as usize];

    let texture = ColorImage::from_rgba_unmultiplied(size, image.as_flat_samples().as_slice());

    ctx.load_texture("background", texture, Default::default())
}

fn set_world_map(ctx: &Context) -> TextureHandle {
    let bytes = image::load_from_memory(include_bytes!("../1920px-Blue_Marble_2002.png")).expect("Did not get access to fallback background").to_rgba8();

    let size = [bytes.width() as usize, bytes.height() as usize];

    let texture = ColorImage::from_rgba_unmultiplied(size, &bytes.as_flat_samples().as_slice());

    ctx.load_texture("world_map", texture, Default::default())
}

#[cfg(test)]
mod tests {
    use super::{snap_font_points, FIXEDSYS_GRID_PX};

    /// The whole point of the snap: whatever comes back must land on a whole
    /// number of pixels at the scale it was snapped for.
    fn assert_whole_pixels(points: f32, ppp: f32) {
        let px = snap_font_points(points, ppp) * ppp;
        assert!(
            (px - px.round()).abs() < 0.01,
            "{points}pt at ppp {ppp} gave {px}px, which is not a whole pixel"
        );
    }

    #[test]
    fn snaps_to_whole_pixels_at_any_scale() {
        for ppp in [1.0, 1.25, 1.5, 1.5625, 1.75, 2.0] {
            for points in [8.0, 11.0, 12.0, 14.0, 18.0, 22.0, 30.0] {
                assert_whole_pixels(points, ppp);
            }
        }
    }

    #[test]
    fn prefers_the_16px_grid_when_it_is_close() {
        // 11pt at 1.5625 is 17.19px — within tolerance of 16, the size Fixedsys
        // is actually drawn at, so it takes it.
        let px = snap_font_points(11.0, 1.5625) * 1.5625;
        assert!((px - FIXEDSYS_GRID_PX).abs() < 0.01, "got {px}px");

        // 30pt at 1x is 30px, near enough to 32 to be worth taking.
        assert!((snap_font_points(30.0, 1.0) - 32.0).abs() < 0.01);
    }

    #[test]
    fn leaves_sizes_alone_when_the_grid_is_far_away() {
        // 18pt at 1.5625 is 28.12px; the nearest grid multiple is 32, a 14%
        // jump — too big a change to make for sharpness, so it settles for the
        // nearest whole pixel instead.
        let px = snap_font_points(18.0, 1.5625) * 1.5625;
        assert!((px - 28.0).abs() < 0.01, "got {px}px");

        // At 1x the common sizes are already whole pixels and stay put.
        assert_eq!(snap_font_points(18.0, 1.0), 18.0);
        assert_eq!(snap_font_points(11.0, 1.0), 11.0);
    }

    #[test]
    fn degenerate_inputs_are_returned_unchanged() {
        assert_eq!(snap_font_points(12.0, 0.0), 12.0);
        assert_eq!(snap_font_points(0.0, 2.0), 0.0);
    }
}
