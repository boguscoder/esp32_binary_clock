use crate::display::{DISPLAY_HEIGHT, DISPLAY_WIDTH};
use crate::time::Time;
use crate::ui::{BG_COLOR, INACTIVE_COLOR};
use embedded_graphics::{
    geometry::Size,
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{
        CornerRadii, PrimitiveStyleBuilder, Rectangle, RoundedRectangle, StrokeAlignment,
    },
};

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

pub fn render_bcd_clock<T>(display: &mut T, time: &Time) -> Result<(), T::Error>
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
