use core::convert::Infallible;

use embedded_hal::blocking::spi::Transfer;
use max7219_dot_matrix::{Command, MAX7219};
use stm32f1xx_hal::gpio::{gpioa::PA4, Output, PushPull};

use crate::SpiError;

// the CS output pin on stm32f1xx_hal is Infallible
pub(crate) type Max7219Error = max7219_dot_matrix::Error<SpiError, Infallible>;
type Max7219<'a> = MAX7219<'a, PA4<Output<PushPull>>>;

#[derive(Debug)]
pub enum LedPanelError {
    Max7219(Max7219Error),
}

pub struct LedPanel<'a> {
    max7219: &'a mut Max7219<'a>,
    spi: &'a mut dyn Transfer<u8, Error = SpiError>,
}

impl From<Max7219Error> for LedPanelError {
    fn from(err: Max7219Error) -> Self {
        LedPanelError::Max7219(err)
    }
}

impl<'a> LedPanel<'a> {
    pub fn new(
        max7219: &'a mut Max7219<'a>,
        spi: &'a mut dyn Transfer<u8, Error = SpiError>,
    ) -> Result<Self, LedPanelError> {
        Ok(LedPanel { max7219, spi })
    }

    fn clear(&mut self) -> Result<(), LedPanelError> {
        // clear the display and set defaults
        self.max7219
            .write_command_all(self.spi, Command::OnOff, 0)?;
        self.max7219
            .write_command_all(self.spi, Command::ScanLimit, 7)?;
        self.max7219
            .write_command_all(self.spi, Command::Intensity, 10)?; // 0-15
        self.max7219
            .write_command_all(self.spi, Command::DecodeMode, 0)?;
        self.max7219
            .write_command_all(self.spi, Command::DisplayTest, 0)?;
        self.max7219.clear_all(self.spi)?;
        self.max7219
            .write_command_all(self.spi, Command::OnOff, 1)?;
        Ok(())
    }

    pub fn scroll_str(&mut self, message: &str) -> Result<(), LedPanelError> {
        self.clear()?;
        let from_pos = self.max7219.get_num_devices() * 8;
        let to_pos = message.len() as i32 * -8;
        let mut pos = from_pos as i32;

        loop {
            pos -= 1;
            self.max7219.write_str_at_pos(self.spi, message, pos)?;

            // done scrolling
            if pos < to_pos {
                return Ok(());
            }
        }
    }
}
