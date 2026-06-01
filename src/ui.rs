use crate::display::{Display, LandscapeDisplay};
use crate::time::{SetMode, Time};
use core::fmt::Write;
use core::net::Ipv4Addr;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embedded_graphics::{
    mono_font::{
        ascii::{FONT_6X10, FONT_9X15_BOLD},
        MonoTextStyleBuilder,
    },
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Circle, PrimitiveStyleBuilder, StrokeAlignment},
    text::{Alignment, Text},
};

const BG_COLOR: Rgb565 = Rgb565::BLACK;
const INACTIVE_COLOR: Rgb565 = Rgb565::new(4, 7, 10); // Dim outline for off bits
const TEXT_MUTED: Rgb565 = Rgb565::new(12, 24, 36); // Slate gray labels
const H_INNER: Rgb565 = Rgb565::new(31, 0, 16);
const H_OUTER: Rgb565 = Rgb565::new(20, 0, 8);
const M_INNER: Rgb565 = Rgb565::new(0, 60, 30);
const M_OUTER: Rgb565 = Rgb565::new(0, 30, 15);
const S_INNER: Rgb565 = Rgb565::new(31, 45, 0);
const S_OUTER: Rgb565 = Rgb565::new(20, 20, 0);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UiType {
    BcdTime,
    FullTime,
    Info,
}

impl UiType {
    pub fn next(self) -> Self {
        match self {
            UiType::BcdTime => UiType::FullTime,
            UiType::FullTime => UiType::Info,
            UiType::Info => UiType::BcdTime,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Info {
    ssid: &'static str,
    connected: bool,
    ip_address: Ipv4Addr,
    timezone_offset: i32,
    dirty: bool,
}

impl Info {
    const fn new() -> Self {
        Self {
            ssid: env!("WIFI_SSID"),
            connected: false,
            ip_address: Ipv4Addr::UNSPECIFIED,
            timezone_offset: 0,
            dirty: true,
        }
    }

    pub fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
        self.dirty = true;
    }

    pub fn set_ip_address(&mut self, ip: Ipv4Addr) {
        self.ip_address = ip;
        self.dirty = true;
    }

    pub fn set_timezone_offset(&mut self, offset: i32) {
        self.timezone_offset = offset;
        self.dirty = true;
    }
}

pub static CURRENT_INFO: Mutex<CriticalSectionRawMutex, Info> = Mutex::new(Info::new());

// Draws a premium glowing dot (orb) on the display
fn draw_glowing_dot<D>(
    target: &mut D,
    center: Point,
    is_on: bool,
    inner_color: Rgb565,
    outer_color: Rgb565,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    const RADIUS: u32 = 8;
    const ERASURE_RADIUS: u32 = RADIUS + 4;

    if is_on {
        // Outer glowing aura
        Circle::new(
            center - Point::new((RADIUS + 3) as i32, (RADIUS + 3) as i32),
            (RADIUS + 3) * 2 + 1,
        )
        .into_styled(PrimitiveStyleBuilder::new().fill_color(outer_color).build())
        .draw(target)?;

        // Inner hot core
        Circle::new(
            center - Point::new(RADIUS as i32, RADIUS as i32),
            RADIUS * 2 + 1,
        )
        .into_styled(PrimitiveStyleBuilder::new().fill_color(inner_color).build())
        .draw(target)?;
    } else {
        // Erase old glowing aura first with background color
        Circle::new(
            center - Point::new(ERASURE_RADIUS as i32, ERASURE_RADIUS as i32),
            ERASURE_RADIUS * 2 + 1,
        )
        .into_styled(PrimitiveStyleBuilder::new().fill_color(BG_COLOR).build())
        .draw(target)?;

        // Muted inactive state (a simple dim circle outline)
        Circle::new(
            center - Point::new(RADIUS as i32, RADIUS as i32),
            RADIUS * 2 + 1,
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

fn render_clock(
    display: &mut LandscapeDisplay,
    time: &Time,
    set_mode: SetMode,
    ui_type: UiType,
    force_redraw: bool,
) {
    if force_redraw {
        let label_style = MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(TEXT_MUTED)
            .build();

        Text::with_alignment("H", Point::new(67, 125), label_style, Alignment::Center)
            .draw(display)
            .unwrap();
        Text::with_alignment("M", Point::new(162, 125), label_style, Alignment::Center)
            .draw(display)
            .unwrap();
        Text::with_alignment("S", Point::new(257, 125), label_style, Alignment::Center)
            .draw(display)
            .unwrap();
    }

    // Flash state for configuration mode (toggles every 250ms -> 2Hz flash rate)
    let is_flash_on = (time.milliseconds / 250).is_multiple_of(2);

    let cols = [
        (time.hours / 10, 2, H_INNER, H_OUTER, SetMode::SetHours),
        (time.hours % 10, 4, H_INNER, H_OUTER, SetMode::SetHours),
        (time.minutes / 10, 3, M_INNER, M_OUTER, SetMode::SetMinutes),
        (time.minutes % 10, 4, M_INNER, M_OUTER, SetMode::SetMinutes),
        (time.seconds / 10, 3, S_INNER, S_OUTER, SetMode::None),
        (time.seconds % 10, 4, S_INNER, S_OUTER, SetMode::None),
    ];

    let x_positions = [45, 90, 140, 185, 235, 280];
    let y_positions = [35, 60, 85, 110];
    let y_shift = if ui_type == UiType::FullTime { 0 } else { 10 };

    for (col_idx, &(val, max_bits, inner_col, outer_col, mode)) in cols.iter().enumerate() {
        let display_column = set_mode != mode || is_flash_on;

        for (row_idx, _) in y_positions.iter().enumerate() {
            let bit_val = 1 << (3 - row_idx);
            let bit_exists = match max_bits {
                2 => bit_val <= 2,
                3 => bit_val <= 4,
                _ => true,
            };

            if bit_exists {
                let is_on = ((val & bit_val) != 0) && display_column;
                let center = Point::new(x_positions[col_idx], y_positions[row_idx] + y_shift);
                draw_glowing_dot(display, center, is_on, inner_col, outer_col).unwrap();
            }
        }
    }

    if ui_type == UiType::FullTime {
        let mut time_str = heapless::String::<12>::new();

        let show_hours = set_mode != SetMode::SetHours || is_flash_on;
        let show_minutes = set_mode != SetMode::SetMinutes || is_flash_on;

        if show_hours {
            let _ = write!(&mut time_str, "{:02}", time.hours);
        } else {
            let _ = time_str.push_str("  ");
        }

        let _ = time_str.push(':');

        if show_minutes {
            let _ = write!(&mut time_str, "{:02}", time.minutes);
        } else {
            let _ = time_str.push_str("  ");
        }

        let _ = time_str.push(':');
        let _ = write!(&mut time_str, "{:02}", time.seconds);

        // Dynamic, self-erasing text block using the theme's text color
        let text_style = MonoTextStyleBuilder::new()
            .font(&FONT_9X15_BOLD)
            .text_color(Rgb565::new(31, 63, 31))
            .background_color(BG_COLOR)
            .build();

        Text::with_alignment(
            time_str.as_str(),
            Point::new(160, 155),
            text_style,
            Alignment::Center,
        )
        .draw(display)
        .unwrap();
    }
}

async fn render_info(display: &mut LandscapeDisplay<'_, '_>) {
    let mut time_str = heapless::String::<128>::new();

    {
        let mut info = CURRENT_INFO.lock().await;
        let _ = write!(
            &mut time_str,
            "SSID: {}\nConnected: {}\nIP: {:?}\nOffset: {}s",
            info.ssid, info.connected, info.ip_address, info.timezone_offset
        );

        if info.dirty {
            display.clear(BG_COLOR).unwrap();
            info.dirty = false;
        }
    }

    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_9X15_BOLD)
        .text_color(Rgb565::new(31, 63, 31))
        .background_color(BG_COLOR)
        .build();

    Text::with_alignment(
        time_str.as_str(),
        Point::new(10, 30),
        text_style,
        Alignment::Left,
    )
    .draw(display)
    .unwrap();
}

pub async fn render_ui(
    raw_display: &mut Display<'_>,
    time: &Time,
    set_mode: SetMode,
    ui_type: UiType,
    force_redraw: bool,
) {
    let mut display_target = LandscapeDisplay { base: raw_display };

    if force_redraw {
        display_target.base.clear(BG_COLOR).unwrap();
    }
    match ui_type {
        UiType::BcdTime | UiType::FullTime => {
            render_clock(&mut display_target, time, set_mode, ui_type, force_redraw)
        }

        UiType::Info => render_info(&mut display_target).await,
    }
}
