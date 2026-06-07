use crate::display::{Display, LandscapeDisplay, DISPLAY_HEIGHT, DISPLAY_WIDTH};
use crate::time::{SetMode, Time};
use crate::time_sync::ConnectionState;
use core::f32::consts::PI;
use core::fmt::Write;
use core::net::Ipv4Addr;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embedded_graphics::{
    geometry::Size,
    mono_font::{ascii::FONT_10X20, MonoTextStyleBuilder},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{
        Arc, CornerRadii, Line, PrimitiveStyleBuilder, Rectangle, RoundedRectangle, StrokeAlignment,
    },
    text::{Alignment, Text},
};
use embedded_graphics_framebuf::FrameBuf;

const BG_COLOR: Rgb565 = Rgb565::BLACK;
const INACTIVE_COLOR: Rgb565 = Rgb565::new(4, 7, 10);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UiType {
    BcdTime,
    RegularTime,
    Info,
}

impl UiType {
    pub fn next(self) -> Self {
        match self {
            UiType::BcdTime => UiType::RegularTime,
            UiType::RegularTime => UiType::Info,
            UiType::Info => UiType::BcdTime,
        }
    }
}

const PADDING: u32 = 2;
const GLOW_SIZE: u32 = 5;
const TOTAL_OFFSET: u32 = GLOW_SIZE + PADDING;
const FULL_PADDING: u32 = TOTAL_OFFSET * 2;
const DOT_SIZE: u32 = (DISPLAY_HEIGHT as u32 / 4) - FULL_PADDING;

fn draw_glowing_square<T>(
    target: &mut T,
    top_left: Point,
    is_on: bool,
    inner_color: Rgb565,
    outer_color: Rgb565,
) -> Result<(), T::Error>
where
    T: DrawTarget<Color = Rgb565>,
{
    const RADIUS: u32 = 8;
    const RADII: CornerRadii = CornerRadii::new(Size::new(RADIUS, RADIUS));
    const ERASURE_DIM: u32 = DOT_SIZE + FULL_PADDING;
    const GLOW_DIM: u32 = DOT_SIZE + (GLOW_SIZE * 2);

    let glow_top_left = top_left + Point::new(PADDING as i32, PADDING as i32);
    let core_top_left = top_left + Point::new(TOTAL_OFFSET as i32, TOTAL_OFFSET as i32);

    if is_on {
        RoundedRectangle::new(
            Rectangle::new(glow_top_left, Size::new(GLOW_DIM, GLOW_DIM)),
            RADII,
        )
        .into_styled(PrimitiveStyleBuilder::new().fill_color(outer_color).build())
        .draw(target)?;

        Rectangle::new(core_top_left, Size::new(DOT_SIZE, DOT_SIZE))
            .into_styled(PrimitiveStyleBuilder::new().fill_color(inner_color).build())
            .draw(target)?;
    } else {
        RoundedRectangle::new(
            Rectangle::new(top_left, Size::new(ERASURE_DIM, ERASURE_DIM)),
            RADII,
        )
        .into_styled(PrimitiveStyleBuilder::new().fill_color(BG_COLOR).build())
        .draw(target)?;
        RoundedRectangle::new(
            Rectangle::new(core_top_left, Size::new(DOT_SIZE, DOT_SIZE)),
            RADII,
        )
        .into_styled(
            PrimitiveStyleBuilder::new()
                .stroke_color(INACTIVE_COLOR)
                .stroke_width(2)
                .stroke_alignment(StrokeAlignment::Inside)
                .build(),
        )
        .draw(target)?;
    }

    Ok(())
}

fn render_bcd_clock<T>(display: &mut T, time: &Time) -> Result<(), T::Error>
where
    T: DrawTarget<Color = Rgb565>,
{
    const COLS: [(i32, Rgb565, Rgb565); 6] = [
        (2, Rgb565::new(31, 0, 16), Rgb565::new(20, 0, 8)),
        (4, Rgb565::new(31, 0, 16), Rgb565::new(20, 0, 8)),
        (3, Rgb565::new(0, 60, 30), Rgb565::new(0, 30, 15)),
        (4, Rgb565::new(0, 60, 30), Rgb565::new(0, 30, 15)),
        (3, Rgb565::new(31, 45, 0), Rgb565::new(20, 20, 0)),
        (4, Rgb565::new(31, 45, 0), Rgb565::new(20, 20, 0)),
    ];
    const SECTION_PADDING: u32 = PADDING * 6;
    const COL_STRIDE: i32 = (DOT_SIZE + FULL_PADDING) as i32;
    const X_SHIFT: i32 = (DISPLAY_WIDTH - (6 * COL_STRIDE)) / 2 - SECTION_PADDING as i32;

    let mut shift = 0;
    let values = [
        time.hours / 10,
        time.hours % 10,
        time.minutes / 10,
        time.minutes % 10,
        time.seconds / 10,
        time.seconds % 10,
    ];

    for (col_idx, (val, (max_bits, inner_col, outer_col))) in values.iter().zip(COLS).enumerate() {
        if col_idx != 0 && col_idx.is_multiple_of(2) {
            shift += SECTION_PADDING
        }

        for row_idx in 0..4 {
            let bit_val = 1 << (3 - row_idx);
            let bit_exists = match max_bits {
                2 => bit_val <= 2,
                3 => bit_val <= 4,
                _ => true,
            };

            if bit_exists {
                let is_on = (val & bit_val) != 0;
                let location = Point::new(
                    X_SHIFT + (col_idx as i32 * COL_STRIDE) + shift as i32,
                    row_idx * COL_STRIDE,
                );
                draw_glowing_square(display, location, is_on, inner_col, outer_col)?
            }
        }
    }

    Ok(())
}

pub fn draw_arc<D>(
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

fn render_arc_clock<T>(display: &mut T, time: &Time, set_mode: SetMode) -> Result<(), T::Error>
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    ssid: &'static str,
    state: ConnectionState,
    ip_address: Option<Ipv4Addr>,
    timezone_name: Option<heapless::String<32>>,
    sync_time: Option<Time>,
}

impl Info {
    const fn new() -> Self {
        Self {
            ssid: env!("WIFI_SSID"),
            state: ConnectionState::Disconnected,
            ip_address: None,
            timezone_name: None,
            sync_time: None,
        }
    }

    pub fn set_state(&mut self, state: ConnectionState) {
        self.state = state;
    }

    pub fn set_ip_address(&mut self, ip: Ipv4Addr) {
        self.ip_address = Some(ip);
    }

    pub fn set_timezone(&mut self, name: heapless::String<32>) {
        self.timezone_name = Some(name);
    }

    pub fn set_sync_time(&mut self, time: Time) {
        self.sync_time = Some(time);
    }
}

pub static CURRENT_INFO: Mutex<CriticalSectionRawMutex, Info> = Mutex::new(Info::new());

struct DisplayOption<'a, T>(&'a Option<T>);

impl<'a, T: core::fmt::Display> core::fmt::Display for DisplayOption<'a, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Some(value) => write!(f, "{}", value),
            None => write!(f, "N/A"),
        }
    }
}

async fn render_info<T>(display: &mut T) -> Result<(), T::Error>
where
    T: DrawTarget<Color = Rgb565>,
{
    let mut time_str = heapless::String::<128>::new();

    {
        let info = CURRENT_INFO.lock().await;
        write!(
            &mut time_str,
            "SSID: {}\nState: {:?}\nIP: {}\nTZ: {}\nSync Time: {}",
            info.ssid,
            info.state,
            DisplayOption(&info.ip_address),
            DisplayOption(&info.timezone_name),
            DisplayOption(&info.sync_time)
        )
        .ok();
    }

    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb565::new(31, 63, 31))
        .background_color(BG_COLOR)
        .build();

    Text::with_alignment(
        time_str.as_str(),
        Point::new(10, 30),
        text_style,
        Alignment::Left,
    )
    .draw(display)?;

    Ok(())
}

pub async fn render_ui(
    raw_display: &mut Display<'_>,
    time: &Time,
    set_mode: SetMode,
    ui_type: UiType,
    clear: bool,
) {
    let mut data = [Rgb565::BLACK; DISPLAY_WIDTH as usize * DISPLAY_HEIGHT as usize];
    let mut fbuf = FrameBuf::new(&mut data, DISPLAY_WIDTH as usize, DISPLAY_HEIGHT as usize);

    if clear {
        fbuf.clear(BG_COLOR).unwrap();
    }
    match ui_type {
        UiType::BcdTime => render_bcd_clock(&mut fbuf, time),
        UiType::RegularTime => render_arc_clock(&mut fbuf, time, set_mode),
        UiType::Info => render_info(&mut fbuf).await,
    };

    let mut hardware_display = LandscapeDisplay { base: raw_display };
    hardware_display.draw_iter(&fbuf).unwrap();
}
