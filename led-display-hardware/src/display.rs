use crate::{SpiError, SpiTransfer};
use core::{cell::RefCell, convert::Infallible};
use embedded_hal::blocking::spi::Transfer;
use max7219_dot_matrix::{Command, MAX7219};
use stm32f1xx_hal::gpio::{gpioa::PA4, Output, PushPull};

// MAX7219 dot matrix board with CS pin PA4
type Max7219Physical<'a> = MAX7219<'a, PA4<Output<PushPull>>>;

// the CS output pin on stm32f1xx_hal is Infallible
type Max7219Error = max7219_dot_matrix::Error<SpiError, Infallible>;

#[derive(Debug)]
pub enum LedPanelError {
    Max7219(Max7219Error),
}

pub struct LedPanel<'a> {
    max7219: &'a mut Max7219Physical<'a>,
    spi: &'a RefCell<dyn Transfer<u8, Error = SpiError>>,
}

impl From<Max7219Error> for LedPanelError {
    fn from(err: Max7219Error) -> Self {
        LedPanelError::Max7219(err)
    }
}

impl<'a> LedPanel<'a> {
    pub fn new(max7219: &'a mut Max7219Physical<'a>, spi: &'a RefCell<SpiTransfer>) -> Self {
        LedPanel { max7219, spi }
    }

    pub fn scroll_str(&mut self, message: &str) -> Result<(), LedPanelError> {
        let spi = &mut *self.spi.borrow_mut();
        clear(self.max7219, spi)?;
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

fn clear<'a>(
    max7219: &mut Max7219Physical<'a>,
    spi: &mut dyn Transfer<u8, Error = SpiError>,
) -> Result<(), LedPanelError> {
    // clear the display and set defaults
    max7219.write_command_all(spi, Command::OnOff, 0)?;
    max7219.write_command_all(spi, Command::ScanLimit, 7)?;
    max7219.write_command_all(spi, Command::Intensity, 10)?; // 0-15
    max7219.write_command_all(spi, Command::DecodeMode, 0)?;
    max7219.write_command_all(spi, Command::DisplayTest, 0)?;
    max7219.clear_all(spi)?;
    max7219.write_command_all(spi, Command::OnOff, 1)?;
    Ok(())
}
