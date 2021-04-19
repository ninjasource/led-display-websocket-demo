use core::borrow::BorrowMut;

use max7219_dot_matrix::{Command, MAX7219};
use stm32f1xx_hal::gpio::{gpioa::PA4, Output, PushPull};

type Max7219Peripheral<'a> = MAX7219<'a, PA4<Output<PushPull>>>;

pub struct LedPanel<'a> {
    max7219: &'a mut Max7219Peripheral<'a>,
    //    spi: SharedSpi<'a>,
}
/*
impl<'a> LedPanel<'a> {
    pub fn new(max7219: Max7219Peripheral<'a>, spi: &'a SharedSpi) -> Self {

        let s = spi.borrow_mut();
        max7219.write_command_all(s, Command::OnOff, 0).unwrap();
        max7219.write_command_all(s, Command::ScanLimit, 7).unwrap();
        max7219
            .write_command_all(s, Command::DecodeMode, 0)
            .unwrap();
        max7219
            .write_command_all(s, Command::DisplayTest, 0)
            .unwrap();
        max7219.clear_all(s).unwrap();
        max7219.write_command_all(s, Command::Intensity, 1).unwrap();
        max7219.write_command_all(s, Command::OnOff, 1).unwrap();
        LedPanel { max7219, spi }
    }
}
*/

impl<'a> LedPanel<'a> {
    pub fn new(max7219: &'a mut Max7219Peripheral<'a>) -> Self {
        LedPanel { max7219 }
    }
}
