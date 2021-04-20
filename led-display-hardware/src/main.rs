#![no_std]
#![no_main]
#![allow(warnings)]

//#[macro_use]
//extern crate lazy_static;

//extern crate panic_itm;

#[macro_use]
extern crate rtt_target;

use rtt_target::{rprintln, rtt_init_print};

use core::{
    borrow::{Borrow, BorrowMut},
    cell::{Cell, RefCell},
    fmt::Arguments,
    str::Utf8Error,
};
use cortex_m::{
    asm,
    interrupt::{self, Mutex},
    itm,
    peripheral::{itm::Stim, ITM},
};
use cortex_m_rt::entry;
use display::LedPanel;
use embedded_hal::blocking::spi::Transfer;
use embedded_hal::digital::v2::OutputPin;
use embedded_hal::{spi::FullDuplex, spi::Mode, spi::Phase, spi::Polarity};
use embedded_websocket as ws;
use max7219_dot_matrix::MAX7219;
use network::{EthernetCard, NetworkError, TcpStream};
use shared_bus::{NullMutex, SpiProxy};
use stm32f1xx_hal::{
    delay::Delay,
    gpio::{
        gpioa::{PA2, PA5, PA6, PA7},
        Alternate, Floating, Input, Output, PushPull,
    },
    pac::SPI1,
    prelude::*,
    spi::Spi,
    stm32,
};
use w5500::{IpAddress, Socket, W5500};
use ws::{
    framer::{Framer, FramerError},
    EmptyRng, WebSocketOptions,
};

mod display;
mod network;

#[derive(Debug)]
enum LedDemoError {
    Io(stm32f1xx_hal::spi::Error),
    WebSocket(ws::Error),
    Utf8Error,
    Max7219,
    Network(NetworkError),
    Framer(FramerError),
}

impl From<FramerError> for LedDemoError {
    fn from(err: FramerError) -> LedDemoError {
        LedDemoError::Framer(err)
    }
}

impl From<stm32f1xx_hal::spi::Error> for LedDemoError {
    fn from(err: stm32f1xx_hal::spi::Error) -> LedDemoError {
        LedDemoError::Io(err)
    }
}

impl From<ws::Error> for LedDemoError {
    fn from(err: ws::Error) -> LedDemoError {
        LedDemoError::WebSocket(err)
    }
}

impl From<NetworkError> for LedDemoError {
    fn from(err: NetworkError) -> LedDemoError {
        LedDemoError::Network(err)
    }
}

impl From<Utf8Error> for LedDemoError {
    fn from(_err: Utf8Error) -> LedDemoError {
        LedDemoError::Utf8Error
    }
}

impl<SpiError, PinError> From<max7219_dot_matrix::Error<SpiError, PinError>> for LedDemoError {
    fn from(_err: max7219_dot_matrix::Error<SpiError, PinError>) -> LedDemoError {
        // FIXME: capture more of the error than this simple variant
        LedDemoError::Max7219
    }
}


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
    let mut cp: cortex_m::Peripherals = cortex_m::Peripherals::take().unwrap();
    let dp = stm32::Peripherals::take().unwrap();

    let mut rcc = dp.RCC.constrain();
    let mut afio = dp.AFIO.constrain(&mut rcc.apb2);
    let mut flash = dp.FLASH.constrain();
    let clocks = rcc.cfgr.freeze(&mut flash.acr);
    let mut delay = Delay::new(cp.SYST, clocks);

    // spi setup
    let mut gpioa = dp.GPIOA.split(&mut rcc.apb2);
    let sck = gpioa.pa5.into_alternate_push_pull(&mut gpioa.crl);
    let miso = gpioa.pa6;
    let mosi = gpioa.pa7.into_alternate_push_pull(&mut gpioa.crl);
    let mut cs_max7219 = gpioa.pa4.into_push_pull_output(&mut gpioa.crl);
    let mut cs_ethernet = gpioa.pa2.into_push_pull_output(&mut gpioa.crl);
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
    delay.delay_ms(250_u16);
    rprintln!("[INF] Done initialising");

    // used to share the spi bus
    let bus = shared_bus::BusManagerSimple::new(spi);

    loop {
        // setup led panel
        let mut max7219 = MAX7219::new(&mut cs_max7219, 20);
        let _led_spi = bus.acquire_spi();
        let mut led_panel = LedPanel::new(&mut max7219);
        //let mut led_panel = LedPanel::new(max7219);

        // setup ethernet card
        let ethernet_spi = &mut bus.acquire_spi();
        let mut w5500 = W5500::new(&mut cs_ethernet, ethernet_spi);
        //let mut network_card = NetworkCard::new(w5500, bus.acquire_spi());

        client_connect(&mut led_panel, &mut w5500).unwrap();
    }
}

fn client_connect<'a>(
    _led_panel: &mut LedPanel,
    w5500: &'a mut EthernetCard<'a>,
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

    let mut stream = TcpStream::new(w5500, Socket::Socket0);
    stream.connect(&host_ip, host_port)?;

    let mut websocket = ws::WebSocketClient::new_client(EmptyRng::new());
    let mut read_buf = [0; 512];
    let mut read_cursor = 0;
    let mut write_buf = [0; 512];
    let mut frame_buf = [0; 4096];
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

    //  log(itm, "[INF] Websocket sending connect handshake");
    framer.connect(&mut stream, &websocket_options)?;
    rprintln!("[INF] Websocket opening handshake complete");
    //    log(itm, "[INF] Websocket connected");
    while let Some(text) = framer.read_text(&mut stream, &mut frame_buf)? {
        //log_fmt(itm, format_args!("[INF] Received: {}", text));

        rprintln!("[INF] Websocket received: {}", text);
        // TODO: log and scroll message
    }

    Ok(())
}
/*
fn scroll_str<PinError, CS>(
    max7219: &mut MAX7219<CS>,
    spi: &mut SpiFullDuplex,
    message: &str,
) -> Result<(), LedDemoError>
where
    CS: OutputPin<Error = PinError>,
{
    let from_pos = max7219.get_num_devices() * 8;
    let to_pos = message.len() as i32 * -8;
    let mut pos = from_pos as i32;

    loop {
        pos -= 1;

        max7219.write_str_at_pos(spi, message, pos)?;

        // start over
        if pos < to_pos {
            return Ok(());
        }
    }
}
*/
