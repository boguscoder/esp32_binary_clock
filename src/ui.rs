use crate::display::{Display, LandscapeDisplay, DISPLAY_HEIGHT, DISPLAY_WIDTH};
use crate::time::{SetMode, Time};
use crate::time_sync::ConnectionState;
use core::fmt::Write;
use core::net::Ipv4Addr;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, MonoTextStyleBuilder},
    pixelcolor::Rgb565,
    prelude::*,
    text::{Alignment, Text},
};
use embedded_graphics_framebuf::FrameBuf;

pub const BG_COLOR: Rgb565 = Rgb565::BLACK;
pub const INACTIVE_COLOR: Rgb565 = Rgb565::new(4, 7, 10);

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

pub static FRAMEBUFFER: Mutex<CriticalSectionRawMutex, [Rgb565; (DISPLAY_WIDTH * DISPLAY_HEIGHT) as usize]> =
    Mutex::new([Rgb565::BLACK; (DISPLAY_WIDTH * DISPLAY_HEIGHT) as usize]);

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
    let mut fb_guard = FRAMEBUFFER.lock().await;
    let mut fbuf = FrameBuf::new(
        &mut *fb_guard,
        DISPLAY_WIDTH as usize,
        DISPLAY_HEIGHT as usize,
    );

    if clear {
        fbuf.clear(BG_COLOR).unwrap();
    }
    match ui_type {
        UiType::BcdTime => crate::ui_bcd::render_bcd_clock(&mut fbuf, time),
        UiType::RegularTime => crate::ui_arc::render_arc_clock(&mut fbuf, time, set_mode),
        UiType::Info => render_info(&mut fbuf).await,
    };

    let mut hardware_display = LandscapeDisplay { base: raw_display };
    hardware_display.draw_iter(&fbuf).unwrap();
}
