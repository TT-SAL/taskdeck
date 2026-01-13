use egui::{self, Align2, Color32, CornerRadius, Rect, Shape, Stroke, StrokeKind, Widget};
use emath;
use epaint::TextShape;
use egui::{FontId, Ui, Pos2, FontFamily, vec2, Sense};

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

        let text_font = FontId::new(11.0, FontFamily::Name("space".into()));

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

        // Layout the text in two lines manually
        let available_text_width = rect.right() - (number_pos.x + number_size.x + margin - 4.0);
        let full_text = self.text;

        // Split text into two lines based on available width
        let (first_line, second_line) = {
            let words = full_text.split_whitespace();
            let mut line1 = String::new();
            let mut line2 = String::new();
            let mut fitting = true;

            for word in words {
                let test_line = if line1.is_empty() {
                    word.to_string()
                } else {
                    format!("{} {}", line1, word)
                };

                let test_width = ui
                    .fonts_mut(|f| f.layout_no_wrap(test_line.clone(), text_font.clone(), ui.visuals().text_color()))
                    .size()
                    .x;

                if test_width <= available_text_width && fitting {
                    line1 = test_line;
                } else {
                    fitting = false;
                    if line2.is_empty() {
                        line2.push_str(word);
                    } else {
                        line2.push_str(&format!(" {}", word));
                    }
                }
            }

            (line1, line2)
        };

        let color = Color32::from_gray(150);

        let text_offset_x = 2.0; // Push text more to the right
        let text_offset_y = 7.5; // Push text a bit lower

        let line1_pos = Pos2::new(
            number_pos.x + number_size.x + margin * 2.0 + text_offset_x,
            number_pos.y + text_offset_y + 1.0,
        );
        let line2_pos = Pos2::new(
            rect.left() + margin + text_offset_x,
            number_pos.y + number_size.y + margin + text_offset_y - 6.0,
        );

        painter.text(line1_pos, Align2::LEFT_TOP, first_line, text_font.clone(), color);
        painter.text(line2_pos, Align2::LEFT_TOP, second_line, text_font.clone(), color);

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

        let text_font = FontId::new(11.0, FontFamily::Name("space".into()));
        let color = Color32::from_gray(150);

        let margin = 12.0;
        let available_text_width = rect.width() - margin * 2.0;

        // Word-wrapping into two lines
        let (line1, line2) = {
            let words = self.text.split_whitespace();
            let mut line1 = String::new();
            let mut line2 = String::new();
            let mut fitting = true;

            for word in words {
                let test = if line1.is_empty() {
                    word.to_string()
                } else {
                    format!("{} {}", line1, word)
                };

                let width = ui.fonts_mut(|f| {
                    f.layout_no_wrap(test.clone(), text_font.clone(), color).size().x
                });

                if width <= available_text_width && fitting {
                    line1 = test;
                } else {
                    fitting = false;
                    if !line2.is_empty() {
                        line2.push(' ');
                    }
                    line2.push_str(word);
                }
            }

            (line1, line2)
        };

        let line_height = 18.0; // Approximate line height
        let line1_pos = Pos2::new(rect.left() + margin, rect.top() + margin);
        let line2_pos = Pos2::new(rect.left() + margin, rect.top() + margin + line_height);

        painter.text(line1_pos, Align2::LEFT_TOP, line1, text_font.clone(), color);
        painter.text(line2_pos, Align2::LEFT_TOP, line2, text_font, color);

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

        let text_font = FontId::new(11.0, FontFamily::Name("space".into()));

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

        //available widths for first and second rows
        let first_row_width = rect.width() - margin * 3f32;
        // let second_row_width = number_center.x - rotated_size.x / 2.0 - rect.left() - margin * 2.0;

        let full_text = self.text;

        // Split text into two lines based on available width
        let (first_line, second_line) = {
            let words = full_text.split_whitespace();
            let mut line1 = String::new();
            let mut line2 = String::new();

            for word in words {
                let test_line = if line1.is_empty() {
                    word.to_string()
                } else {
                    format!("{} {}", line1, word)
                };

                let test_width = ui
                    .fonts_mut(|f| f.layout_no_wrap(test_line.clone(), text_font.clone(), ui.visuals().text_color()))
                    .size()
                    .x;

                if test_width < first_row_width {
                    line1 = test_line;
                } else {
                    line2.push_str(word);
                }
            }

            (line1, line2)
        };

        let color = Color32::from_gray(150);

        // Position text on top-left, with some margin
        let text_offset_x = margin + 7.0;
        let text_offset_y = margin + 1.0;

        let line1_pos = Pos2::new(rect.left() + text_offset_x, rect.top() + text_offset_y);
        let line2_pos = Pos2::new(
            rect.left() + text_offset_x,
            rect.top() + text_offset_y + text_font.size + 2.0,
        );

        painter.text(line1_pos, Align2::LEFT_TOP, first_line, text_font.clone(), color);
        painter.text(line2_pos, Align2::LEFT_TOP, second_line, text_font, color);



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

        let text_font = FontId::new(11.0, FontFamily::Name("space".into()));

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

        //available widths for first and second rows
        let first_row_width = rect.width() - margin * 3f32;
        // let second_row_width = number_center.x - rotated_size.x / 2.0 - rect.left() - margin * 2.0;

        let full_text = self.text;

        // Split text into two lines based on available width
        let (first_line, second_line) = {
            let words = full_text.split_whitespace();
            let mut line1 = String::new();
            let mut line2 = String::new();

            for word in words {
                let test_line = if line1.is_empty() {
                    word.to_string()
                } else {
                    format!("{} {}", line1, word)
                };

                let test_width = ui
                    .fonts_mut(|f| f.layout_no_wrap(test_line.clone(), text_font.clone(), ui.visuals().text_color()))
                    .size()
                    .x;

                if test_width < first_row_width {
                    line1 = test_line;
                } else {
                    line2.push_str(word);
                }
            }

            (line1, line2)
        };

        let color = Color32::from_gray(150);

        // Position text on top-left, with some margin
        let text_offset_x = margin + 7.0;
        let text_offset_y = margin + 1.0;

        let line1_pos = Pos2::new(rect.left() + text_offset_x, rect.top() + text_offset_y);
        let line2_pos = Pos2::new(
            rect.left() + text_offset_x,
            rect.top() + text_offset_y + text_font.size + 2.0,
        );

        painter.text(line1_pos, Align2::LEFT_TOP, first_line, text_font.clone(), color);
        painter.text(line2_pos, Align2::LEFT_TOP, second_line, text_font, color);


        let button_size = vec2(30.0, 18.0);
        let button_pos = Pos2::new(rect.left() + margin, rect.bottom() - margin - button_size.y);
        let button_rect = Rect::from_min_size(button_pos, button_size);

        ui.allocate_ui_at_rect(button_rect, |ui| {
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