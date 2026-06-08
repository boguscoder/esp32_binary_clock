use crate::display::{DISPLAY_HEIGHT, DISPLAY_WIDTH};
use crate::time::{SetMode, Time};
use crate::ui::{BG_COLOR, INACTIVE_COLOR};
use core::f32::consts::PI;
use core::fmt::Write;
use embedded_graphics::{
    geometry::Size,
    mono_font::{ascii::FONT_10X20, MonoTextStyleBuilder},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Arc, Line, PrimitiveStyleBuilder, Rectangle},
    text::{Alignment, Text},
};

fn draw_arc<D>(
    display: &mut D,
    bounds: Rectangle,
    radius: u32,
    thickness: u32,
    progress: f32,
    active_color: Rgb565,
    track_color: Rgb565,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    if bounds.size.width <= thickness * 2 || bounds.size.height <= thickness * 2 {
        return Ok(());
    }

    let progress = progress.clamp(0.0, 1.0);

    let style_active = PrimitiveStyleBuilder::new()
        .stroke_color(active_color)
        .stroke_width(thickness)
        .build();

    let style_track = PrimitiveStyleBuilder::new()
        .stroke_color(track_color)
        .stroke_width(thickness)
        .build();

    let half_thick = thickness as f32 / 2.0;

    let safe_radius = (radius as f32)
        .min(bounds.size.width as f32 / 2.0)
        .min(bounds.size.height as f32 / 2.0);

    let w_straight = (bounds.size.width as f32 - (2.0 * safe_radius)).max(0.0);
    let h_straight = (bounds.size.height as f32 - (2.0 * safe_radius)).max(0.0);

    let center_radius = (safe_radius - half_thick).max(1.0);
    let arc_diameter = (center_radius * 2.0) as u32;
    let arc_len = 0.5 * PI * center_radius;

    enum SegType {
        TopRightLine,
        TopRightArc,
        RightLine,
        BottomRightArc,
        BottomLine,
        BottomLeftArc,
        LeftLine,
        TopLeftArc,
        TopLeftLine,
    }

    let segments = [
        (SegType::TopRightLine, w_straight / 2.0),
        (SegType::TopRightArc, arc_len),
        (SegType::RightLine, h_straight),
        (SegType::BottomRightArc, arc_len),
        (SegType::BottomLine, w_straight),
        (SegType::BottomLeftArc, arc_len),
        (SegType::LeftLine, h_straight),
        (SegType::TopLeftArc, arc_len),
        (SegType::TopLeftLine, w_straight / 2.0),
    ];

    let total_perimeter: f32 = segments.iter().map(|s| s.1).sum();

    if total_perimeter <= 0.1 {
        return Ok(());
    }

    let mut active_budget = progress * total_perimeter;

    let x_left = bounds.top_left.x as f32 + half_thick;
    let x_right = (bounds.top_left.x + bounds.size.width as i32) as f32 - half_thick;
    let y_top = bounds.top_left.y as f32 + half_thick;
    let y_bottom = (bounds.top_left.y + bounds.size.height as i32) as f32 - half_thick;

    for (seg_type, length) in segments {
        if length <= 0.001 {
            continue;
        }

        let seg_active_len = active_budget.clamp(0.0, length);
        let seg_track_len = length - seg_active_len;

        match seg_type {
            SegType::TopRightLine => {
                let start_pt = Point::new(
                    (x_left + w_straight / 2.0 + center_radius) as i32,
                    y_top as i32,
                );
                if seg_active_len > 0.0 {
                    let end = Point::new((start_pt.x as f32 + seg_active_len) as i32, y_top as i32);
                    Line::new(start_pt, end)
                        .into_styled(style_active)
                        .draw(display)?;
                }
                if seg_track_len > 0.0 {
                    let start =
                        Point::new((start_pt.x as f32 + seg_active_len) as i32, y_top as i32);
                    let end = Point::new((start.x as f32 + seg_track_len) as i32, y_top as i32);
                    Line::new(start, end)
                        .into_styled(style_track)
                        .draw(display)?;
                }
            }
            SegType::TopRightArc => {
                let arc_top_left = Point::new((x_right - 2.0 * center_radius) as i32, y_top as i32);
                let start_angle = 270.0;
                if seg_active_len > 0.0 {
                    let sweep = (seg_active_len / length) * 90.0;
                    Arc::new(arc_top_left, arc_diameter, start_angle.deg(), sweep.deg())
                        .into_styled(style_active)
                        .draw(display)?;
                }
                if seg_track_len > 0.0 {
                    let active_sweep = (seg_active_len / length) * 90.0;
                    let sweep = (seg_track_len / length) * 90.0;
                    Arc::new(
                        arc_top_left,
                        arc_diameter,
                        (start_angle + active_sweep).deg(),
                        sweep.deg(),
                    )
                    .into_styled(style_track)
                    .draw(display)?;
                }
            }
            SegType::RightLine => {
                let start_pt = Point::new(x_right as i32, (y_top + center_radius) as i32);
                if seg_active_len > 0.0 {
                    let end =
                        Point::new(x_right as i32, (start_pt.y as f32 + seg_active_len) as i32);
                    Line::new(start_pt, end)
                        .into_styled(style_active)
                        .draw(display)?;
                }
                if seg_track_len > 0.0 {
                    let start =
                        Point::new(x_right as i32, (start_pt.y as f32 + seg_active_len) as i32);
                    let end = Point::new(x_right as i32, (start.y as f32 + seg_track_len) as i32);
                    Line::new(start, end)
                        .into_styled(style_track)
                        .draw(display)?;
                }
            }
            SegType::BottomRightArc => {
                let arc_top_left = Point::new(
                    (x_right - 2.0 * center_radius) as i32,
                    (y_bottom - 2.0 * center_radius) as i32,
                );
                let start_angle = 0.0;
                if seg_active_len > 0.0 {
                    let sweep = (seg_active_len / length) * 90.0;
                    Arc::new(arc_top_left, arc_diameter, start_angle.deg(), sweep.deg())
                        .into_styled(style_active)
                        .draw(display)?;
                }
                if seg_track_len > 0.0 {
                    let active_sweep = (seg_active_len / length) * 90.0;
                    let sweep = (seg_track_len / length) * 90.0;
                    Arc::new(
                        arc_top_left,
                        arc_diameter,
                        (start_angle + active_sweep).deg(),
                        sweep.deg(),
                    )
                    .into_styled(style_track)
                    .draw(display)?;
                }
            }
            SegType::BottomLine => {
                let start_pt = Point::new((x_right - center_radius) as i32, y_bottom as i32);
                if seg_active_len > 0.0 {
                    let end =
                        Point::new((start_pt.x as f32 - seg_active_len) as i32, y_bottom as i32);
                    Line::new(start_pt, end)
                        .into_styled(style_active)
                        .draw(display)?;
                }
                if seg_track_len > 0.0 {
                    let start =
                        Point::new((start_pt.x as f32 - seg_active_len) as i32, y_bottom as i32);
                    let end = Point::new((start.x as f32 - seg_track_len) as i32, y_bottom as i32);
                    Line::new(start, end)
                        .into_styled(style_track)
                        .draw(display)?;
                }
            }
            SegType::BottomLeftArc => {
                let arc_top_left =
                    Point::new(x_left as i32, (y_bottom - 2.0 * center_radius) as i32);
                let start_angle = 90.0;
                if seg_active_len > 0.0 {
                    let sweep = (seg_active_len / length) * 90.0;
                    Arc::new(arc_top_left, arc_diameter, start_angle.deg(), sweep.deg())
                        .into_styled(style_active)
                        .draw(display)?;
                }
                if seg_track_len > 0.0 {
                    let active_sweep = (seg_active_len / length) * 90.0;
                    let sweep = (seg_track_len / length) * 90.0;
                    Arc::new(
                        arc_top_left,
                        arc_diameter,
                        (start_angle + active_sweep).deg(),
                        sweep.deg(),
                    )
                    .into_styled(style_track)
                    .draw(display)?;
                }
            }
            SegType::LeftLine => {
                let start_pt = Point::new(x_left as i32, (y_bottom - center_radius) as i32);
                if seg_active_len > 0.0 {
                    let end =
                        Point::new(x_left as i32, (start_pt.y as f32 - seg_active_len) as i32);
                    Line::new(start_pt, end)
                        .into_styled(style_active)
                        .draw(display)?;
                }
                if seg_track_len > 0.0 {
                    let start =
                        Point::new(x_left as i32, (start_pt.y as f32 - seg_active_len) as i32);
                    let end = Point::new(x_left as i32, (start.y as f32 - seg_track_len) as i32);
                    Line::new(start, end)
                        .into_styled(style_track)
                        .draw(display)?;
                }
            }
            SegType::TopLeftArc => {
                let arc_top_left = Point::new(x_left as i32, y_top as i32);
                let start_angle = 180.0;
                if seg_active_len > 0.0 {
                    let sweep = (seg_active_len / length) * 90.0;
                    Arc::new(arc_top_left, arc_diameter, start_angle.deg(), sweep.deg())
                        .into_styled(style_active)
                        .draw(display)?;
                }
                if seg_track_len > 0.0 {
                    let active_sweep = (seg_active_len / length) * 90.0;
                    let sweep = (seg_track_len / length) * 90.0;
                    Arc::new(
                        arc_top_left,
                        arc_diameter,
                        (start_angle + active_sweep).deg(),
                        sweep.deg(),
                    )
                    .into_styled(style_track)
                    .draw(display)?;
                }
            }
            SegType::TopLeftLine => {
                let start_pt = Point::new((x_left + center_radius) as i32, y_top as i32);
                if seg_active_len > 0.0 {
                    let end = Point::new((start_pt.x as f32 + seg_active_len) as i32, y_top as i32);
                    Line::new(start_pt, end)
                        .into_styled(style_active)
                        .draw(display)?;
                }
                if seg_track_len > 0.0 {
                    let start =
                        Point::new((start_pt.x as f32 + seg_active_len) as i32, y_top as i32);
                    let end = Point::new((start.x as f32 + seg_track_len) as i32, y_top as i32);
                    Line::new(start, end)
                        .into_styled(style_track)
                        .draw(display)?;
                }
            }
        }
        active_budget = (active_budget - length).max(0.0);
    }
    Ok(())
}

pub fn render_arc_clock<T>(display: &mut T, time: &Time, set_mode: SetMode) -> Result<(), T::Error>
where
    T: DrawTarget<Color = Rgb565>,
{
    let gap = 3;
    let thickness = 18;
    let base_radius = 60;
    let p1 = 6;

    draw_arc(
        display,
        Rectangle::new(
            Point::new(p1, p1),
            Size::new(
                (DISPLAY_WIDTH - p1 * 2) as u32,
                (DISPLAY_HEIGHT - p1 * 2) as u32,
            ),
        ),
        base_radius, // 52
        thickness,
        time.hours as f32 / 24.0,
        Rgb565::new(31, 0, 16),
        INACTIVE_COLOR,
    )?;

    let p2 = p1 + thickness as i32 + gap;
    let r2 = base_radius - (thickness + gap as u32);

    draw_arc(
        display,
        Rectangle::new(
            Point::new(p2, p2),
            Size::new(
                (DISPLAY_WIDTH - p2 * 2) as u32,
                (DISPLAY_HEIGHT - p2 * 2) as u32,
            ),
        ),
        r2,
        thickness,
        time.minutes as f32 / 60.0,
        Rgb565::new(0, 60, 30),
        INACTIVE_COLOR,
    )?;

    let p3 = p2 + thickness as i32 + gap;
    let r3 = base_radius - ((thickness + gap as u32) * 2);

    draw_arc(
        display,
        Rectangle::new(
            Point::new(p3, p3),
            Size::new(
                (DISPLAY_WIDTH - p3 * 2) as u32,
                (DISPLAY_HEIGHT - p3 * 2) as u32,
            ),
        ),
        r3,
        thickness,
        time.seconds as f32 / 60.0,
        Rgb565::new(31, 45, 0),
        INACTIVE_COLOR,
    )?;

    let is_flash_on = (time.milliseconds / 250).is_multiple_of(2);
    let mut time_str = heapless::String::<12>::new();

    let show_hours = set_mode != SetMode::SetHours || is_flash_on;
    let show_minutes = set_mode != SetMode::SetMinutes || is_flash_on;

    if show_hours {
        write!(&mut time_str, "{:02}", time.hours).ok();
    } else {
        time_str.push_str("  ").ok();
    }

    time_str.push(':').ok();

    if show_minutes {
        write!(&mut time_str, "{:02}", time.minutes).ok();
    } else {
        time_str.push_str("  ").ok();
    }

    time_str.push(':').ok();
    write!(&mut time_str, "{:02}", time.seconds).ok();

    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb565::new(31, 63, 31))
        .background_color(BG_COLOR)
        .build();

    Text::with_alignment(
        time_str.as_str(),
        Point::new(160, DISPLAY_HEIGHT / 2 + 5),
        text_style,
        Alignment::Center,
    )
    .draw(display)?;
    Ok(())
}
