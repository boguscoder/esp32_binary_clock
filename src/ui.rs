use crate::display::{Display, LandscapeDisplay, DISPLAY_HEIGHT, DISPLAY_WIDTH};
use crate::time::{SetMode, Time};
use crate::time_sync::ConnectionState;
use core::fmt::Write;
use core::net::Ipv4Addr;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embedded_graphics::{
    mono_font::{ascii::FONT_9X15_BOLD, MonoTextStyleBuilder},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyleBuilder, Rectangle, StrokeAlignment},
    text::{Alignment, Text},
};

const BG_COLOR: Rgb565 = Rgb565::BLACK;
const INACTIVE_COLOR: Rgb565 = Rgb565::new(4, 7, 10); // Dim outline for off bits
const H_INNER: Rgb565 = Rgb565::new(31, 0, 16);
const H_OUTER: Rgb565 = Rgb565::new(20, 0, 8);
const M_INNER: Rgb565 = Rgb565::new(0, 60, 30);
const M_OUTER: Rgb565 = Rgb565::new(0, 30, 15);
const S_INNER: Rgb565 = Rgb565::new(31, 45, 0);
const S_OUTER: Rgb565 = Rgb565::new(20, 20, 0);

const GLOW_SIZE: u32 = 4;
const PADDING: u32 = 2;
const SECTION_PADDING: u32 = PADDING * 6;

const TOTAL_OFFSET: u32 = GLOW_SIZE + PADDING;
const FULL_PADDING: u32 = TOTAL_OFFSET * 2;

const DOT_SIZE: u32 = (DISPLAY_HEIGHT as u32 / 4) - FULL_PADDING;
const ERASURE_DIM: u32 = DOT_SIZE + FULL_PADDING;
const GLOW_DIM: u32 = DOT_SIZE + (GLOW_SIZE * 2);

const COL_STRIDE: i32 = (DOT_SIZE + FULL_PADDING) as i32;
const X_SHIFT: i32 = (DISPLAY_WIDTH - (6 * COL_STRIDE)) / 2 - SECTION_PADDING as i32;

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

fn draw_glowing_square<D>(
    target: &mut D,
    top_left: Point,
    is_on: bool,
    inner_color: Rgb565,
    outer_color: Rgb565,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    let glow_top_left = top_left + Point::new(PADDING as i32, PADDING as i32);
    let core_top_left = top_left + Point::new(TOTAL_OFFSET as i32, TOTAL_OFFSET as i32);

    if is_on {
        Rectangle::new(glow_top_left, Size::new(GLOW_DIM, GLOW_DIM))
            .into_styled(PrimitiveStyleBuilder::new().fill_color(outer_color).build())
            .draw(target)?;

        Rectangle::new(core_top_left, Size::new(DOT_SIZE, DOT_SIZE))
            .into_styled(PrimitiveStyleBuilder::new().fill_color(inner_color).build())
            .draw(target)?;
    } else {
        Rectangle::new(top_left, Size::new(ERASURE_DIM, ERASURE_DIM))
            .into_styled(PrimitiveStyleBuilder::new().fill_color(BG_COLOR).build())
            .draw(target)?;

        Rectangle::new(core_top_left, Size::new(DOT_SIZE, DOT_SIZE))
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

fn render_clock(display: &mut LandscapeDisplay, time: &Time, set_mode: SetMode, ui_type: UiType) {
    // Flash state for configuration mode (toggles every 250ms -> 2Hz flash rate)
    let is_flash_on = (time.milliseconds / 250).is_multiple_of(2);
    let mut shift_section = 0;
    let cols = [
        (time.hours / 10, 2, H_INNER, H_OUTER, SetMode::SetHours),
        (time.hours % 10, 4, H_INNER, H_OUTER, SetMode::SetHours),
        (time.minutes / 10, 3, M_INNER, M_OUTER, SetMode::SetMinutes),
        (time.minutes % 10, 4, M_INNER, M_OUTER, SetMode::SetMinutes),
        (time.seconds / 10, 3, S_INNER, S_OUTER, SetMode::None),
        (time.seconds % 10, 4, S_INNER, S_OUTER, SetMode::None),
    ];

    for (col_idx, &(val, max_bits, inner_col, outer_col, mode)) in cols.iter().enumerate() {
        let display_column = set_mode != mode || is_flash_on;

        if col_idx != 0 && col_idx.is_multiple_of(2) {
            shift_section += SECTION_PADDING
        }

        for row_idx in 0..4 {
            let bit_val = 1 << (3 - row_idx);
            let bit_exists = match max_bits {
                2 => bit_val <= 2,
                3 => bit_val <= 4,
                _ => true,
            };

            if bit_exists {
                let is_on = ((val & bit_val) != 0) && display_column;
                let location = Point::new(
                    X_SHIFT + (col_idx as i32 * COL_STRIDE) + shift_section as i32,
                    row_idx * COL_STRIDE,
                );
                draw_glowing_square(display, location, is_on, inner_col, outer_col).unwrap();
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

async fn render_info(display: &mut LandscapeDisplay<'_, '_>) {
    let mut time_str = heapless::String::<128>::new();

    {
        let info = CURRENT_INFO.lock().await;
        let _ = write!(
            &mut time_str,
            "SSID: {}\nState: {:?}\nIP: {}\nTZ: {}\nSync Time: {}",
            info.ssid,
            info.state,
            DisplayOption(&info.ip_address),
            DisplayOption(&info.timezone_name),
            DisplayOption(&info.sync_time)
        );
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
            render_clock(&mut display_target, time, set_mode, ui_type)
        }

        UiType::Info => render_info(&mut display_target).await,
    }
}
