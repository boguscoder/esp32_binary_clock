use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    pixelcolor::{Rgb565, RgbColor},
    Pixel,
};
use embedded_hal::delay::DelayNs;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::{
    delay::Delay,
    gpio::{AnyPin, Level, Output, OutputConfig, Pin},
    ledc::{
        channel, channel::ChannelIFace, timer, timer::TimerIFace, LSGlobalClkSource, Ledc, LowSpeed,
    },
    spi::master::{Config as SpiConfig, Spi},
};
use mipidsi::{
    interface::SpiInterface,
    models::ST7789,
    options::{ColorInversion, Orientation, Rotation},
    Builder,
};
use static_cell::StaticCell;

pub type Display<'d> = mipidsi::Display<
    SpiInterface<'d, ExclusiveDevice<Spi<'d, esp_hal::Blocking>, Output<'d>, Delay>, Output<'d>>,
    ST7789,
    Output<'d>,
>;

pub struct LandscapeDisplay<'a, 'd> {
    pub base: &'a mut Display<'d>,
}

impl<'a, 'd> DrawTarget for LandscapeDisplay<'a, 'd> {
    type Color = Rgb565;
    type Error = <Display<'d> as DrawTarget>::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        const DISPLAY_HEIGHT: i32 = 172;
        const OFFSET_X: i32 = 34; // Discover hardware offset from custom display

        self.base
            .draw_iter(pixels.into_iter().map(|Pixel(Point { x, y }, color)| {
                // Map logical landscape (x, y) to physical portrait (st_x, st_y)
                let st_x = (DISPLAY_HEIGHT - 1 - y) + OFFSET_X;
                let st_y = x;
                Pixel(Point::new(st_x, st_y), color)
            }))
    }
}

impl<'a, 'd> OriginDimensions for LandscapeDisplay<'a, 'd> {
    fn size(&self) -> Size {
        Size::new(320, 172)
    }
}

pub struct DisplayConfig<'d> {
    pub spi: esp_hal::peripherals::SPI2<'d>,
    pub mosi: AnyPin<'d>,
    pub sclk: AnyPin<'d>,
    pub cs: AnyPin<'d>,
    pub dc: AnyPin<'d>,
    pub rst: AnyPin<'d>,
    pub bl: AnyPin<'d>,
    pub ledc: esp_hal::peripherals::LEDC<'d>,
    pub backlight_duty: u8,
}

pub fn init_display<'d>(config: DisplayConfig<'d>, delay: &mut Delay) -> Display<'d> {
    let mut ledc = Ledc::new(config.ledc);
    ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);

    let mut timer = ledc.timer::<LowSpeed>(timer::Number::Timer0);
    timer
        .configure(timer::config::Config {
            duty: timer::config::Duty::Duty8Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: esp_hal::time::Rate::from_khz(5),
        })
        .unwrap();

    let mut channel = ledc.channel::<LowSpeed>(
        channel::Number::Channel0,
        config.bl.degrade().into_output_signal(),
    );
    channel
        .configure(channel::config::Config {
            timer: &timer,
            duty_pct: config.backlight_duty,
            drive_mode: esp_hal::gpio::DriveMode::PushPull,
        })
        .unwrap();

    let mut rst = Output::new(config.rst, Level::Low, OutputConfig::default());
    delay.delay_ms(100u32);
    rst.set_high();
    delay.delay_ms(100u32);

    let spi_config = SpiConfig::default().with_frequency(esp_hal::time::Rate::from_mhz(40));
    let spi = Spi::new(config.spi, spi_config)
        .expect("Failed to init SPI")
        .with_sck(config.sclk)
        .with_mosi(config.mosi);
    let cs = Output::new(config.cs, Level::High, OutputConfig::default());
    let dc = Output::new(config.dc, Level::Low, OutputConfig::default());

    let spi_device = ExclusiveDevice::new(spi, cs, Delay::new()).unwrap();
    static DISPLAY_BUFFER: StaticCell<[u8; 1024]> = StaticCell::new();
    let di = SpiInterface::new(spi_device, dc, DISPLAY_BUFFER.init([0; 1024]));

    let mut display = Builder::new(ST7789, di)
        .display_size(240, 320)
        .display_offset(0, 0)
        .invert_colors(ColorInversion::Inverted)
        .reset_pin(rst)
        .init(delay)
        .unwrap();

    display
        .set_orientation(Orientation::new().rotate(Rotation::Deg0))
        .unwrap();
    display.clear(Rgb565::BLACK).unwrap();

    display
}
