use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Ticker};
use esp_backtrace as _;
use esp_hal::gpio::{AnyPin, Input, InputConfig, Pull};

#[derive(Clone, Copy)]
pub enum ButtonEvent {
    ShortPress,
    LongPress,
}

pub static BUTTON_EVENTS: Signal<CriticalSectionRawMutex, ButtonEvent> = Signal::new();

#[embassy_executor::task]
pub async fn button_task(button_pin: AnyPin<'static>) {
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
