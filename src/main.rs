#![no_std]
#![no_main]

mod display;
mod time;
mod time_sync;
mod ui;

use crate::display::{init_display, DisplayConfig};
use crate::time::{SetMode, Time};
use crate::ui::{render_ui, UiType, CURRENT_INFO};

use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Ticker};
use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    gpio::{AnyPin, Input, InputConfig, Pull},
    timer::timg::TimerGroup,
};

esp_bootloader_esp_idf::esp_app_desc!();

const BACKLIGHT_BRIGHTNESS: u8 = 10;

#[derive(Clone, Copy)]
enum ButtonEvent {
    ShortPress,
    LongPress,
}

static BUTTON_EVENTS: Signal<CriticalSectionRawMutex, ButtonEvent> = Signal::new();

#[embassy_executor::task]
async fn button_task(button_pin: AnyPin<'static>) {
    const TICK_INTERVAL_MS: u64 = 20;
    let mut ticker = Ticker::every(Duration::from_millis(TICK_INTERVAL_MS));

    let button = Input::new(button_pin, InputConfig::default().with_pull(Pull::Up));

    let mut button_pressed_duration_ms = 0u32;
    let mut last_button_state = button.is_high();

    loop {
        let current_button_state = button.is_high();

        if !current_button_state {
            button_pressed_duration_ms += TICK_INTERVAL_MS as u32;
        } else {
            if !last_button_state {
                if button_pressed_duration_ms >= 1000 {
                    BUTTON_EVENTS.signal(ButtonEvent::LongPress);
                } else if button_pressed_duration_ms >= 30 {
                    BUTTON_EVENTS.signal(ButtonEvent::ShortPress);
                }
                button_pressed_duration_ms = 0;
            }
        }
        last_button_state = current_button_state;

        ticker.next().await;
    }
}

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

    const TICK_INTERVAL_MS: u64 = 20;
    let mut loop_ticker = Ticker::every(Duration::from_millis(TICK_INTERVAL_MS));
    let mut force_redraw = true;
    let mut last_second = 99u8;
    let mut last_flash_state = false;
    let mut last_set_mode = set_mode;
    let mut last_type = display_type;
    let mut last_info = CURRENT_INFO.lock().await.clone();

    loop {
        time = time_sync::CURRENT_TIME.try_take().unwrap_or(time);

        time.tick(TICK_INTERVAL_MS);

        while let Some(event) = BUTTON_EVENTS.try_take() {
            match event {
                ButtonEvent::LongPress => {
                    set_mode = set_mode.next();
                    if set_mode != SetMode::None {
                        display_type = UiType::FullTime;
                    }
                    force_redraw = true;
                }
                ButtonEvent::ShortPress => {
                    match set_mode {
                        SetMode::None => {
                            display_type = display_type.next();
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
            }
        }

        let current_flash_state = (time.milliseconds / 250).is_multiple_of(2);

        let needs_redraw = force_redraw
            || time.seconds != last_second
            || set_mode != last_set_mode
            || display_type != last_type
            || (set_mode != SetMode::None && current_flash_state != last_flash_state);

        if needs_redraw {
            let clear_screen = force_redraw
                || set_mode != last_set_mode
                || display_type != last_type
                || CURRENT_INFO.lock().await.ne(&last_info);

            render_ui(&mut display, &time, set_mode, display_type, clear_screen).await;

            last_second = time.seconds;
            last_flash_state = current_flash_state;
            last_set_mode = set_mode;
            last_type = display_type;
            force_redraw = false;
            last_info = CURRENT_INFO.lock().await.clone();
        }

        loop_ticker.next().await;
    }
}
