#![no_std]
#![no_main]
//#![allow(warnings)]

#[macro_use]
extern crate rtt_target;

mod bearssl;
use bearssl::*;

use crate::network::{
    build_trust_anchor_ta0, build_trust_anchor_ta1, sock_read, sock_write, ENTROPY, FRAME_BUF,
    IO_BUF, NETWORK_HOST, READ_BUF, WRITE_BUF,
};
use core::mem::MaybeUninit;

use core::cell::RefCell;
use cortex_m::asm;
use cortex_m_rt::entry;
use display::{LedPanel, LedPanelError};
use embedded_hal::{blocking::spi::Transfer, spi::Mode, spi::Phase, spi::Polarity};
use embedded_websocket as ws;
use max7219_dot_matrix::MAX7219;
use network::{Connection, EthContext, NetworkError, SslStream};
use rtt_target::{rprintln, rtt_init_print};
use stm32f1xx_hal::{
    delay::Delay,
    gpio::{
        gpioa::{PA5, PA6, PA7},
        Alternate, Floating, Input, PushPull,
    },
    pac::SPI1,
    prelude::*,
    spi::{Spi, Spi1NoRemap},
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
    Display(LedPanelError),
    Network(NetworkError),
    Framer(FramerError<NetworkError>),
}

impl From<LedPanelError> for LedDemoError {
    fn from(err: LedPanelError) -> LedDemoError {
        LedDemoError::Display(err)
    }
}

impl From<FramerError<NetworkError>> for LedDemoError {
    fn from(err: FramerError<NetworkError>) -> LedDemoError {
        LedDemoError::Framer(err)
    }
}

impl From<NetworkError> for LedDemoError {
    fn from(err: NetworkError) -> LedDemoError {
        LedDemoError::Network(err)
    }
}

type SpiError = stm32f1xx_hal::spi::Error;
type SpiTransfer = dyn Transfer<u8, Error = SpiError>;
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
    let mut led_panel = LedPanel::new(&mut max7219, &spi);

    loop {
        let mut connection = Connection::new(Socket::Socket0);
        rprintln!("[INF] Initialising ssl client");

        // **************************************************** SSL INIT ******************************************************
        // NOTE: I had trouble putting this INIT functionality into its own function because of all the pointers flying around.
        // ********************************************************************************************************************

        rprintln!("[INF] building trust anchors");
        let trust_anchors: [br_x509_trust_anchor; 2] =
            [build_trust_anchor_ta0(), build_trust_anchor_ta1()];
        let mut client_context =
            unsafe { MaybeUninit::<br_ssl_client_context>::uninit().assume_init() };
        let mut x509 = unsafe { MaybeUninit::<br_x509_minimal_context>::uninit().assume_init() };
        let mut io_context = unsafe { MaybeUninit::<br_sslio_context>::uninit().assume_init() };

        // client init
        unsafe {
            br_ssl_client_init_full(
                &mut client_context as *mut _,
                &mut x509 as *mut _,
                trust_anchors.as_ptr(),
                trust_anchors.len(),
            )
        };
        rprintln!(
            "[INF] br_ssl_client_init_full: Err: {}",
            client_context.eng.err
        );

        // inject entropy
        unsafe {
            br_ssl_engine_inject_entropy(
                &mut client_context.eng as *mut _,
                (&ENTROPY).as_ptr() as *const cty::c_void,
                ENTROPY.len(),
            )
        };
        rprintln!(
            "[INF] br_ssl_engine_inject_entropy: Err: {}",
            client_context.eng.err
        );

        // set internal IO buffer
        unsafe {
            br_ssl_engine_set_buffer(
                &mut client_context.eng as *mut _,
                &mut IO_BUF as *mut _ as *mut cty::c_void,
                IO_BUF.len(),
                0, // half duplex
            )
        };
        rprintln!(
            "[INF] br_ssl_engine_set_buffer: Err: {}",
            client_context.eng.err
        );

        // reset client in preparation for connection
        unsafe {
            br_ssl_client_reset(
                &mut client_context as *mut _,
                NETWORK_HOST.as_ptr() as *const u8,
                0,
            )
        };
        rprintln!("[INF] br_ssl_client_reset: Err: {}", client_context.eng.err);

        // init ssl IO
        let mut context = EthContext {
            w5500: &mut w5500 as *mut _,
            connection: &mut connection as *mut _,
            spi: &spi as *const _,
            delay: &delay as *const _,
            client_context: &mut client_context as *mut _,
        };

        let context_ptr = &mut context as *mut _ as *mut cty::c_void;
        unsafe {
            br_sslio_init(
                &mut io_context as *mut _,
                &mut client_context.eng as *mut _,
                Some(sock_read),
                context_ptr,
                Some(sock_write),
                context_ptr,
            )
        };

        rprintln!("[INF] br_sslio_init: Err: {}", client_context.eng.err);

        // ********************************************************************************************************************
        // ********************************************* END OF SSL INIT ******************************************************
        // ********************************************************************************************************************

        let mut stream = SslStream::new(
            &mut w5500,
            &mut connection,
            &spi,
            &delay,
            &mut io_context as *mut _,
            &mut client_context as *mut _,
        );

        match client_connect(&mut led_panel, &mut stream) {
            Ok(()) => rprintln!("[INF] Connection closed"),
            Err(error) => rprintln!("[ERR] {:?}", &error),
        }

        // stop further processing
        // comment out the line below this should keep attempting to connect
        loop {}

        // if you uncomment the loop above you should add this delay to prevent the NTP server from being spammed
        // let d = &mut *delay.borrow_mut();
        // d.delay_ms(1000_u16);
    }
}

fn client_connect<'a>(
    led_panel: &mut LedPanel,
    stream: &mut SslStream<'a>,
) -> Result<(), LedDemoError> {
    rprintln!("[INF] Client connecting");

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

    let mut websocket = ws::WebSocketClient::new_client(EmptyRng::new());
    let mut read_cursor = 0;
    let mut framer = Framer::new(
        unsafe { &mut READ_BUF },
        &mut read_cursor,
        unsafe { &mut WRITE_BUF },
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
    framer.connect(stream, &websocket_options)?;
    rprintln!("[INF] Websocket opening handshake complete");

    let frame_buf = unsafe { &mut FRAME_BUF };

    // read one message at a time and display it
    while let Some(message) = framer.read_text(stream, frame_buf)? {
        rprintln!("[INF] Websocket received: {}", message);
        led_panel.scroll_str(message)?;
    }

    Ok(())
}
