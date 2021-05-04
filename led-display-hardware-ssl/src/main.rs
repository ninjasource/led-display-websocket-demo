#![no_std]
#![no_main]
//#![allow(warnings)]

#[macro_use]
extern crate rtt_target;

mod bearssl;
mod ssl;

use core::{cell::RefCell, convert::Infallible};
use cortex_m::asm;
use cortex_m_rt::entry;
use display::{LedPanel, LedPanelError};
use embedded_hal::{spi::Mode, spi::Phase, spi::Polarity};
use embedded_websocket as ws;
use max7219_dot_matrix::MAX7219;
use rtt_target::{rprintln, rtt_init_print};
use ssl::SslError;
use stm32f1xx_hal::{
    delay::Delay,
    gpio::{
        gpioa::{PA2, PA5, PA6, PA7},
        Alternate, Floating, Input, Output, PushPull,
    },
    pac::SPI1,
    prelude::*,
    spi::{Spi, Spi1NoRemap},
    stm32,
};
use tcp::TcpError;
use w5500::{IpAddress, Socket, W5500};
use ws::{
    framer::{Framer, FramerError},
    EmptyRng, WebSocketOptions,
};

use crate::{ssl::SslStream, tcp::TcpStream};

mod display;
mod tcp;
mod time;

#[derive(Debug)]
enum LedDemoError {
    Display(LedPanelError),
    Tcp(TcpError),
    Framer(FramerError<SslError>),
}

impl From<LedPanelError> for LedDemoError {
    fn from(err: LedPanelError) -> LedDemoError {
        LedDemoError::Display(err)
    }
}

impl From<FramerError<SslError>> for LedDemoError {
    fn from(err: FramerError<SslError>) -> LedDemoError {
        LedDemoError::Framer(err)
    }
}

impl From<TcpError> for LedDemoError {
    fn from(err: TcpError) -> LedDemoError {
        LedDemoError::Tcp(err)
    }
}

// Spi port 1
type SpiPhysical = Spi<
    SPI1,
    Spi1NoRemap,
    (
        PA5<Alternate<PushPull>>,
        PA6<Input<Floating>>,
        PA7<Alternate<PushPull>>,
    ),
    u8,
>;

type SpiError = stm32f1xx_hal::spi::Error;

// W5500 ethernet card with CS pin PA2
type W5500Physical = W5500<PA2<Output<PushPull>>>;

// the CS output pin on stm32f1xx_hal is Infallible
type W5500Error = w5500::Error<SpiError, Infallible>;

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
    let mut delay = Delay::new(cp.SYST, clocks);

    // spi setup
    let mut gpioa = dp.GPIOA.split(&mut rcc.apb2);
    let sck = gpioa.pa5.into_alternate_push_pull(&mut gpioa.crl);
    let miso = gpioa.pa6;
    let mosi = gpioa.pa7.into_alternate_push_pull(&mut gpioa.crl);
    let mut cs_max7219 = gpioa.pa4.into_push_pull_output(&mut gpioa.crl);
    let cs_ethernet = gpioa.pa2.into_push_pull_output(&mut gpioa.crl);
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
    delay.delay_ms(250_u16);
    let delay = RefCell::new(delay);

    rprintln!("[INF] Done initialising");

    let spi = RefCell::new(spi);
    let mut w5500 = W5500::new(cs_ethernet);
    let mut max7219 = MAX7219::new(&mut cs_max7219, 20);
    let mut led_panel = LedPanel::new(&mut max7219, &spi, &delay);

    loop {
        rprintln!("[INF] Initialising ssl client");
        let stream = TcpStream::new(&mut w5500, Socket::Socket0, &delay, &spi);

        match client_connect(&mut led_panel, stream) {
            Ok(()) => rprintln!("[INF] Connection closed"),
            Err(error) => rprintln!("[ERR] {:?}", &error),
        }

        // stop further processing
        // comment out the line below this should keep attempting to connect
        loop {}

        // if you uncomment the loop above you should add this delay to prevent the
        // NTP time server from being spammed if there is a recurring error
        // let d = &mut *delay.borrow_mut();
        // d.delay_ms(1000_u16);
    }
}

fn client_connect(led_panel: &mut LedPanel, mut stream: TcpStream) -> Result<(), LedDemoError> {
    rprintln!("[INF] Client connecting");
    let mut read_buf: [u8; 512] = [0; 512];
    let mut write_buf: [u8; 512] = [0; 512];
    let mut frame_buf: [u8; 128] = [0; 128];
    // remote connection
    //let host_ip = IpAddress::new(51, 140, 68, 75);
    //let host_port = 80;
    //let host = "ninjametal.com";
    //let origin = "http://ninjametal.com";

    // remote connection ssl
    let host_ip = IpAddress::new(51, 140, 68, 75);
    let host_port = 443;
    let host = "ninjametal.com";
    let origin = "https://ninjametal.com";

    // local connection
    // let host_ip = IpAddress::new(192, 168, 1, 149);
    // let host_port = 1337;
    // let host = "192.168.1.149";
    // let origin = "http://192.168.1.149";

    // open tcp stream
    stream.connect(&host_ip, host_port)?;

    let mut ssl_stream = SslStream::new(stream);
    ssl_stream.init();

    let mut websocket = ws::WebSocketClient::new_client(EmptyRng::new());
    let mut read_cursor = 0;

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

    rprintln!("[INF] Websocket sending opening handshake");

    // send websocket open handshake
    framer.connect(&mut ssl_stream, &websocket_options)?;
    rprintln!("[INF] Websocket opening handshake complete");

    // read one message at a time and display it
    while let Some(message) = framer.read_text(&mut ssl_stream, &mut frame_buf)? {
        rprintln!("[INF] Websocket received: {}", message);
        led_panel.scroll_str(message)?;
    }

    Ok(())
}
