#![no_std]
#![no_main]
//#![allow(warnings)]

#[macro_use]
extern crate rtt_target;

use core::cell::RefCell;
use cortex_m::asm;
use cortex_m_rt::entry;
use display::{LedPanel, LedPanelError};
use embedded_hal::{spi::Mode, spi::Phase, spi::Polarity};
use embedded_websocket as ws;
use max7219_dot_matrix::MAX7219;
use network::{EthernetCard, NetworkError, TcpStream};
use rtt_target::{rprintln, rtt_init_print};
use stm32f1xx_hal::{delay::Delay, prelude::*, spi::Spi, stm32};
use w5500::{IpAddress, Socket, W5500};
use ws::{
    framer::{Framer, FramerError},
    EmptyRng, WebSocketOptions,
};

mod display;
mod network;

#[derive(Debug)]
enum LedDemoError {
    Spi(SpiError),
    Display(LedPanelError),
    Network(NetworkError),
    Framer(FramerError),
}

impl From<LedPanelError> for LedDemoError {
    fn from(err: LedPanelError) -> LedDemoError {
        LedDemoError::Display(err)
    }
}

impl From<FramerError> for LedDemoError {
    fn from(err: FramerError) -> LedDemoError {
        LedDemoError::Framer(err)
    }
}

impl From<SpiError> for LedDemoError {
    fn from(err: SpiError) -> LedDemoError {
        LedDemoError::Spi(err)
    }
}

impl From<NetworkError> for LedDemoError {
    fn from(err: NetworkError) -> LedDemoError {
        LedDemoError::Network(err)
    }
}

type SpiError = stm32f1xx_hal::spi::Error;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    rprintln!("{}", info);
    loop {
        asm::bkpt() // halt = exit probe-run
    }
}

#[entry]
fn main() -> ! {
    rtt_init_print!();
    rprintln!("[INF] Initializing");

    // general peripheral setup
    let cp: cortex_m::Peripherals = cortex_m::Peripherals::take().unwrap();
    let dp = stm32::Peripherals::take().unwrap();

    let mut rcc = dp.RCC.constrain();
    let mut afio = dp.AFIO.constrain(&mut rcc.apb2);
    let mut flash = dp.FLASH.constrain();
    let clocks = rcc.cfgr.freeze(&mut flash.acr);
    let delay = Delay::new(cp.SYST, clocks);

    let delay = RefCell::new(delay);

    // spi setup
    let mut gpioa = dp.GPIOA.split(&mut rcc.apb2);
    let sck = gpioa.pa5.into_alternate_push_pull(&mut gpioa.crl);
    let miso = gpioa.pa6;
    let mosi = gpioa.pa7.into_alternate_push_pull(&mut gpioa.crl);
    let mut cs_max7219 = gpioa.pa4.into_push_pull_output(&mut gpioa.crl);
    let mut cs_ethernet = gpioa.pa2.into_push_pull_output(&mut gpioa.crl);
    let spi = Spi::spi1(
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
    delay.borrow_mut().delay_ms(250_u16);
    rprintln!("[INF] Done initialising");

    // used to share the spi bus
    let bus = shared_bus::BusManagerSimple::new(spi);

    loop {
        // setup led panel
        let mut max7219 = MAX7219::new(&mut cs_max7219, 20);
        let mut led_spi = bus.acquire_spi();
        let mut led_panel = match LedPanel::new(&mut max7219, &mut led_spi) {
            Ok(x) => x,
            Err(error) => {
                rprintln!("[ERR] LedPanel: {:?}", &error);
                continue;
            }
        };

        // setup ethernet card
        let ethernet_spi = &mut bus.acquire_spi();
        let mut w5500 = W5500::new(&mut cs_ethernet, ethernet_spi);

        match client_connect(&mut led_panel, &mut w5500, &delay) {
            Ok(()) => rprintln!("[INF] Connection closed"),
            Err(error) => rprintln!("[ERR] {:?}", &error),
        }
    }
}

fn client_connect<'a>(
    led_panel: &mut LedPanel,
    w5500: &'a mut EthernetCard<'a>,
    delay: &'a RefCell<Delay>,
) -> Result<(), LedDemoError> {
    rprintln!("[INF] Client connecting");

    let host_ip = IpAddress::new(51, 140, 68, 75);
    let host_port = 80;
    let host = "ninjametal.com";
    let origin = "http://ninjametal.com";

    //let host_ip = IpAddress::new(192, 168, 1, 149);
    //let host_port = 1337;
    //let host = "192.168.1.149";
    //let origin = "http://192.168.1.149";

    let mut stream = TcpStream::new(w5500, Socket::Socket0, delay);
    stream.connect(&host_ip, host_port)?;

    let mut websocket = ws::WebSocketClient::new_client(EmptyRng::new());
    let mut read_buf = [0; 512];
    let mut read_cursor = 0;
    let mut write_buf = [0; 512];
    let mut frame_buf = [0; 1024];
    let mut framer = Framer::new(
        &mut read_buf,
        &mut read_cursor,
        &mut write_buf,
        &mut websocket,
    );

    let websocket_options = WebSocketOptions {
        path: "/ws/ledpanel",
        host,
        origin,
        sub_protocols: None,
        additional_headers: None,
    };

    framer.connect(&mut stream, &websocket_options)?;
    rprintln!("[INF] Websocket opening handshake complete");

    // read one message at a time and display it
    while let Some(message) = framer.read_text(&mut stream, &mut frame_buf)? {
        rprintln!("[INF] Websocket received: {}", message);

        // NOTE: a delay causes the crash too when we receive more than one frame without going back to "Waiting for bytes"
        // _delay.delay_ms(2000_u16);

        led_panel.scroll_str(message)?;
    }

    Ok(())
}
