#![no_std]
#![no_main]

extern crate panic_itm;

#[macro_use]
extern crate cortex_m;

use cortex_m_rt::entry;
use embedded_hal::digital::v2::OutputPin;
use embedded_hal::{spi::FullDuplex, spi::Mode, spi::Phase, spi::Polarity};
use max7219::{Command, MAX7219};
use stm32f1xx_hal::{prelude::*, delay::Delay, spi::Spi, stm32};

#[entry]
fn main() -> ! {
    let mut cp: cortex_m::Peripherals = cortex_m::Peripherals::take().unwrap();
    let dp = stm32::Peripherals::take().unwrap();
    let itm = &mut cp.ITM;
    iprintln!(&mut itm.stim[0], "[INF] Initializing");

    let mut rcc = dp.RCC.constrain();
    let mut afio = dp.AFIO.constrain(&mut rcc.apb2);
    let mut flash = dp.FLASH.constrain();
    let clocks = rcc.cfgr.freeze(&mut flash.acr);
    let mut delay = Delay::new(cp.SYST, clocks);
    let mut gpioa = dp.GPIOA.split(&mut rcc.apb2);

    let sck = gpioa.pa5.into_alternate_push_pull(&mut gpioa.crl);
    let miso = gpioa.pa6;
    let mosi = gpioa.pa7.into_alternate_push_pull(&mut gpioa.crl);
    let mut cs = gpioa.pa4.into_push_pull_output(&mut gpioa.crl);

    let mut spi = Spi::spi1(
        dp.SPI1,
        (sck, miso, mosi),
        &mut afio.mapr,
        Mode {
            polarity: Polarity::IdleLow,
            phase: Phase::CaptureOnFirstTransition,
        },
        2.mhz(), // up to 10mhz for max7219 module, 2mhz is max for bluepill
        clocks,
        &mut rcc.apb2,
    );

    // wait for things to settle
    delay.delay_ms(20_u16);

    let mut max7219 = MAX7219::new(&mut cs, 20);
    iprintln!(&mut itm.stim[0], "[INF] Done initializing");

    // this should never return unless there is a failure
    run(&mut max7219, &mut spi, &mut delay).unwrap();

    iprintln!(&mut itm.stim[0], "[WRN] Unexpected end of run loop");
    loop {}
}

fn run<SpiError, PinError, CS>(
    max7219: &mut MAX7219<CS>,
    spi: &mut dyn FullDuplex<u8, Error = SpiError>,
    delay: &mut Delay,
) -> Result<(), max7219::Error<SpiError, PinError>>
where
    CS: OutputPin<Error = PinError>,
{
    max7219.write_command_all(spi, Command::OnOff, 0)?;
    max7219.write_command_all(spi, Command::ScanLimit, 7)?;
    max7219.write_command_all(spi, Command::DecodeMode, 0)?;
    max7219.write_command_all(spi, Command::DisplayTest, 0)?;
    max7219.clear_all(spi)?;
    max7219.write_command_all(spi, Command::Intensity, 1)?;
    max7219.write_command_all(spi, Command::OnOff, 1)?;

    loop {
        scroll_str(max7219, spi, "An ode to dwb", delay)?;
        delay.delay_ms(1000_u16);
        scroll_str(max7219, spi, "Let me see, where to start.", delay)?;
        delay.delay_ms(500_u16);
        scroll_str(max7219, spi, "Is it art?", delay)?;
        delay.delay_ms(400_u16);
        scroll_str(
            max7219,
            spi,
            "Yes you may say.        Yes it is art!",
            delay,
        )?;
        delay.delay_ms(250_u16);
        scroll_str(max7219, spi, "Close your eyes and imagine it...", delay)?;
        delay.delay_ms(250_u16);
        scroll_str(
            max7219,
            spi,
            "For this you are a part.           \x01 \x01 \x01",
            delay,
        )?;
        delay.delay_ms(1500_u16);
        scroll_str(max7219, spi, "Nandos chicken makes us smarter", delay)?;
        delay.delay_ms(250_u16);
        scroll_str(max7219, spi, "but we do miss our dear friend Artur", delay)?;
        delay.delay_ms(10000_u16);
    }
}

fn scroll_str<SpiError, PinError, CS>(
    max7219: &mut MAX7219<CS>,
    spi: &mut dyn FullDuplex<u8, Error = SpiError>,
    message: &str,
    delay: &mut Delay,
) -> Result<(), max7219::Error<SpiError, PinError>>
where
    CS: OutputPin<Error = PinError>,
{
    let from_pos = max7219.get_num_devices() * 8;
    let to_pos = message.len() as i32 * -8;
    let mut pos = from_pos as i32;

    loop {
        pos -= 1;

        max7219.write_str_at_pos(spi, message, pos)?;

        // delay between frames
        delay.delay_ms(2_u16);

        // start over
        if pos < to_pos {
            return Ok(());
        }
    }
}
