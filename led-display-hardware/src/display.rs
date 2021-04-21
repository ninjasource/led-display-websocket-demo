use core::convert::Infallible;

use embedded_hal::blocking::spi::Transfer;
use max7219_dot_matrix::{Command, MAX7219};
use stm32f1xx_hal::gpio::{gpioa::PA4, Output, PushPull};

use crate::SpiError;

// the CS output pin on stm32f1xx_hal is Infallible
type Max7219Error = max7219_dot_matrix::Error<SpiError, Infallible>;
type Max7219<'a> = MAX7219<'a, PA4<Output<PushPull>>>;

pub struct LedPanel<'a> {
    max7219: &'a mut Max7219<'a>,
}

#[derive(Debug)]
pub enum DisplayError {
    Max7219(Max7219Error),
}

impl From<Max7219Error> for DisplayError {
    fn from(err: Max7219Error) -> DisplayError {
        DisplayError::Max7219(err)
    }
}

impl<'a> LedPanel<'a> {
    pub fn new<SPI>(max7219: &'a mut Max7219<'a>, spi: &mut SPI) -> Result<Self, DisplayError>
    where
        SPI: Transfer<u8, Error = SpiError>,
    {
        max7219.write_command_all(spi, Command::OnOff, 0)?;
        max7219.write_command_all(spi, Command::ScanLimit, 7)?;
        max7219.write_command_all(spi, Command::DecodeMode, 0)?;
        max7219.write_command_all(spi, Command::DisplayTest, 0)?;
        max7219.clear_all(spi)?;
        max7219.write_command_all(spi, Command::OnOff, 1)?;
        Ok(LedPanel { max7219 })
    }

    pub fn scroll_str<SPI>(&mut self, spi: &mut SPI, message: &str) -> Result<(), DisplayError>
    where
        SPI: Transfer<u8, Error = SpiError>,
    {
        let from_pos = self.max7219.get_num_devices() * 8;
        let to_pos = message.len() as i32 * -8;
        let mut pos = from_pos as i32;

        loop {
            pos -= 1;

            self.max7219.write_str_at_pos(spi, message, pos)?;

            // start over
            if pos < to_pos {
                // log(itm, "[DBG] Done scrolling message");
                return Ok(());
            }
        }
    }
}
