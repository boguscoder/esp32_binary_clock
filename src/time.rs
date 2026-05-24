#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Time {
    pub hours: u8,
    pub minutes: u8,
    pub seconds: u8,
    pub milliseconds: u16,
}

impl Time {
    pub fn new(hours: u8, minutes: u8, seconds: u8) -> Self {
        Self {
            hours,
            minutes,
            seconds,
            milliseconds: 0,
        }
    }

    pub fn tick(&mut self, elapsed_ms: u64) {
        let total_ms = self.milliseconds as u64 + elapsed_ms;
        self.milliseconds = (total_ms % 1000) as u16;

        let total_seconds = self.seconds as u64 + (total_ms / 1000);
        self.seconds = (total_seconds % 60) as u8;

        let total_minutes = self.minutes as u64 + (total_seconds / 60);
        self.minutes = (total_minutes % 60) as u8;

        let total_hours = self.hours as u64 + (total_minutes / 60);
        self.hours = (total_hours % 24) as u8;
    }

    pub fn increment_hour(&mut self) {
        self.hours = (self.hours + 1) % 24;
    }

    pub fn increment_minute(&mut self) {
        self.minutes = (self.minutes + 1) % 60;
        self.seconds = 0; // Reset seconds when setting minutes for precision
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum SetMode {
    None,
    SetHours,
    SetMinutes,
}

impl SetMode {
    pub fn next(self) -> Self {
        match self {
            SetMode::None => SetMode::SetHours,
            SetMode::SetHours => SetMode::SetMinutes,
            SetMode::SetMinutes => SetMode::None,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum DisplayType {
    BcdOnly,
    Full,
}

impl DisplayType {
    pub fn next(self) -> Self {
        match self {
            DisplayType::BcdOnly => DisplayType::Full,
            DisplayType::Full => DisplayType::BcdOnly,
        }
    }
}
