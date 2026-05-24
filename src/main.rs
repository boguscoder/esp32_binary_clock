#![no_std]
#![no_main]

mod display;
mod time;
mod ui;

use crate::display::{init_display, DisplayConfig};
use crate::time::{DisplayType, SetMode, Time};
use crate::ui::render_ui;
use embedded_hal::delay::DelayNs;
use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    gpio::{Input, InputConfig, Pull},
    time::Instant,
};

esp_bootloader_esp_idf::esp_app_desc!();

const BACKLIGHT_BRIGHTNESS: u8 = 10;

#[esp_hal::main]
fn main() -> ! {
    let config = esp_hal::Config::default();
    let peripherals = esp_hal::init(config);
    let mut delay = Delay::new();

    let button = Input::new(
        peripherals.GPIO9,
        InputConfig::default().with_pull(Pull::Up),
    );

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

    let mut time = Time::new(12, 0, 0);
    let mut set_mode = SetMode::None;
    let mut displayType = DisplayType::BcdOnly;

    let mut last_update_time = Instant::now();
    let mut button_pressed_duration_ms = 0u32;
    let mut last_button_state = button.is_high();

    let mut force_redraw = true;
    let mut last_second = 99u8;
    let mut last_flash_state = false;
    let mut last_set_mode = set_mode;
    let mut last_type = displayType;

    loop {
        let now = Instant::now();
        let elapsed_ms = (now - last_update_time).as_millis();
        last_update_time = now;

        time.tick(elapsed_ms);

        // Button debouncer
        let current_button_state = button.is_high();
        if !current_button_state {
            button_pressed_duration_ms += elapsed_ms as u32;
        } else {
            if !last_button_state {
                if button_pressed_duration_ms >= 1000 {
                    // Long press
                    set_mode = set_mode.next();
                    if set_mode != SetMode::None {
                        displayType = DisplayType::Full;
                    }
                    force_redraw = true;
                } else if button_pressed_duration_ms >= 30 {
                    // Short press
                    match set_mode {
                        SetMode::None => {
                            displayType = displayType.next();
                        }
                        SetMode::SetHours => {
                            time.increment_hour();
                        }
                        SetMode::SetMinutes => {
                            time.increment_minute();
                        }
                    }
                    force_redraw = true;
                }
                button_pressed_duration_ms = 0;
            }
        }
        last_button_state = current_button_state;

        let current_flash_state = (time.milliseconds / 250).is_multiple_of(2);

        let needs_redraw = force_redraw
            || time.seconds != last_second
            || set_mode != last_set_mode
            || displayType != last_type
            || (set_mode != SetMode::None && current_flash_state != last_flash_state);

        if needs_redraw {
            let clear_screen =
                force_redraw || set_mode != last_set_mode || displayType != last_type;

            render_ui(&mut display, &time, set_mode, displayType, clear_screen);

            last_second = time.seconds;
            last_flash_state = current_flash_state;
            last_set_mode = set_mode;
            last_type = displayType;
            force_redraw = false;
        }

        delay.delay_ms(20u32);
    }
}
