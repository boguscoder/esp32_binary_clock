#![no_std]
#![no_main]

mod button;
mod display;
mod time;
mod time_sync;
mod ui;

use crate::button::{button_task, ButtonEvent, BUTTON_EVENTS};
use crate::display::{init_display, DisplayConfig};
use crate::time::{SetMode, Time};
use crate::ui::{render_ui, UiType, CURRENT_INFO};

use embassy_executor::Spawner;
use embassy_time::{Duration, Ticker};
use esp_backtrace as _;
use esp_hal::{delay::Delay, timer::timg::TimerGroup};

esp_bootloader_esp_idf::esp_app_desc!();

const BACKLIGHT_BRIGHTNESS: u8 = 5;
const TICK_INTERVAL_MS: u64 = 20;

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    esp_alloc::heap_allocator!(size: 96 * 1024);

    let config = esp_hal::Config::default();
    let peripherals = esp_hal::init(config);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_intr =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_intr.software_interrupt0);

    time_sync::setup_time_sync(peripherals.WIFI, spawner);
    spawner.spawn(button_task(peripherals.GPIO9.into()).unwrap());

    let mut delay = Delay::new();
    let mut display = init_display(
        DisplayConfig {
            spi: peripherals.SPI2,
            mosi: peripherals.GPIO6.into(),
            sclk: peripherals.GPIO7.into(),
            cs: peripherals.GPIO14.into(),
            dc: peripherals.GPIO15.into(),
            rst: peripherals.GPIO21.into(),
            bl: peripherals.GPIO22.into(),
            ledc: peripherals.LEDC,
            backlight_duty: BACKLIGHT_BRIGHTNESS,
        },
        &mut delay,
    );

    let mut time = time_sync::CURRENT_TIME
        .try_take()
        .unwrap_or(Time::new(12, 0, 0));
    let mut set_mode = SetMode::None;
    let mut display_type = UiType::BcdTime;
    let mut loop_ticker = Ticker::every(Duration::from_millis(TICK_INTERVAL_MS));
    let mut last_second = 99u8;
    let mut last_flash_state = false;
    let mut last_info = CURRENT_INFO.lock().await.clone();

    loop {
        time = time_sync::CURRENT_TIME.try_take().unwrap_or(time);

        time.tick(TICK_INTERVAL_MS);

        let button_clicked = BUTTON_EVENTS.signaled();

        while let Some(event) = BUTTON_EVENTS.try_take() {
            match event {
                ButtonEvent::LongPress => {
                    set_mode = set_mode.next();
                    if set_mode != SetMode::None {
                        display_type = UiType::FullTime;
                    }
                }
                ButtonEvent::ShortPress => match set_mode {
                    SetMode::None => {
                        display_type = display_type.next();
                    }
                    SetMode::SetHours => {
                        time.increment_hour();
                    }
                    SetMode::SetMinutes => {
                        time.increment_minute();
                    }
                },
            }
        }

        let current_flash_state = (time.milliseconds / 250).is_multiple_of(2);

        let needs_redraw = button_clicked
            || time.seconds != last_second
            || (set_mode != SetMode::None && current_flash_state != last_flash_state);

        if needs_redraw {
            let new_info = CURRENT_INFO.lock().await.clone();
            let clear_screen = button_clicked || new_info != last_info;

            render_ui(&mut display, &time, set_mode, display_type, clear_screen).await;

            last_second = time.seconds;
            last_flash_state = current_flash_state;
            last_info = new_info;
        }

        loop_ticker.next().await;
    }
}
