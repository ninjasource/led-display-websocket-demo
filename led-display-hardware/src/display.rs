use core::convert::Infallible;

use embedded_hal::blocking::spi::Transfer;
use max7219_dot_matrix::{Command, MAX7219};
use stm32f1xx_hal::gpio::{gpioa::PA4, Output, PushPull};

use crate::SpiError;

// the CS output pin on stm32f1xx_hal is Infallible
pub(crate) type Max7219Error = max7219_dot_matrix::Error<SpiError, Infallible>;
type Max7219<'a> = MAX7219<'a, PA4<Output<PushPull>>>;

pub struct LedPanel<'a> {
    max7219: &'a mut Max7219<'a>,
}

impl<'a> LedPanel<'a> {
    pub fn new(
        max7219: &'a mut Max7219<'a>,
        spi: &mut dyn Transfer<u8, Error = SpiError>,
    ) -> Result<Self, Max7219Error> {
        // clear the display and set defaults
        max7219.write_command_all(spi, Command::OnOff, 0)?;
        max7219.write_command_all(spi, Command::ScanLimit, 7)?;
        max7219.write_command_all(spi, Command::DecodeMode, 0)?;
        max7219.write_command_all(spi, Command::DisplayTest, 0)?;
        max7219.clear_all(spi)?;
        max7219.write_command_all(spi, Command::OnOff, 1)?;
        Ok(LedPanel { max7219 })
    }

    pub fn scroll_str(
        &mut self,
        spi: &mut dyn Transfer<u8, Error = SpiError>,
        message: &str,
    ) -> Result<(), Max7219Error> {
        let from_pos = self.max7219.get_num_devices() * 8;
        let to_pos = message.len() as i32 * -8;
        let mut pos = from_pos as i32;

        loop {
            pos -= 1;

            self.max7219.write_str_at_pos(spi, message, pos)?;

            // done scrolling
            if pos < to_pos {
                return Ok(());
            }
        }
    }
}
