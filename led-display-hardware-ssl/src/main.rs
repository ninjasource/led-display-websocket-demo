//#![cfg(feature = "spin_no_std")]
#![no_std]
#![no_main]
#![allow(warnings)]

#[macro_use]
extern crate rtt_target;

#[macro_use]
extern crate lazy_static;

mod bearssl;
use bearssl::*;

use core::{fmt::Arguments, mem::MaybeUninit};
use cty::size_t;

use core::cell::RefCell;
use cortex_m::asm;
use cortex_m_rt::entry;
use display::{LedPanel, LedPanelError};
use embedded_hal::{blocking::spi::Transfer, spi::Mode, spi::Phase, spi::Polarity};
use embedded_websocket as ws;
use max7219_dot_matrix::MAX7219;
use network::{NetworkError, TcpStream};
use rtt_target::{rprintln, rtt_init_print};
use stm32f1xx_hal::{delay::Delay, prelude::*, spi::Spi, stm32};
use w5500::{IpAddress, Socket, W5500};
use ws::{
    framer::{Framer, FramerError, Stream},
    EmptyRng, WebSocketOptions,
};

mod display;
mod network;

#[derive(Debug)]
enum LedDemoError {
    Display(LedPanelError),
    Network(NetworkError),
    Framer(FramerError<NetworkError>),
    Ssl(FramerError<SslError>),
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

impl From<FramerError<SslError>> for LedDemoError {
    fn from(err: FramerError<SslError>) -> LedDemoError {
        LedDemoError::Ssl(err)
    }
}

type SpiError = stm32f1xx_hal::spi::Error;
type SpiTransfer = dyn Transfer<u8, Error = SpiError>;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    rprintln!("{}", info);
    loop {
        asm::bkpt() // halt = exit probe-run
    }
}

// ************ SSL Related ********************

#[derive(Debug)]
enum SslError {
    WriteBrErr(i32),
    ReadBrErr(i32),
}

struct Ssl {
    cc: br_ssl_client_context,
    ioc: br_sslio_context,
}

// no mangle so that the linker can find this function
// which will be called from BearSSL
#[no_mangle]
extern "C" fn time(_time: &bearssl::__time_t) -> bearssl::__time_t {
    1591000000
}

// no mangle so that the linker can find this function
// which will be called from BearSSL
#[no_mangle]
extern "C" fn strlen(s: &str) -> usize {
    s.len()
}

unsafe extern "C" fn sock_read(
    read_context: *mut cty::c_void,
    data: *mut cty::c_uchar,
    len: size_t,
) -> cty::c_int {
    let stream: &mut TcpStream = &mut *(read_context as *mut TcpStream);
    let buf: &mut [u8] = core::slice::from_raw_parts_mut(data, len as usize);

    let max_len = if buf.len() > len as usize {
        len as usize
    } else {
        buf.len()
    };

    // TODO: figure out what to do if this panics
    let size = stream.read(&mut buf[..max_len]).unwrap();
    //let size = 0;
    rprintln!("[DBG] sock_read received {} bytes", size);
    return size as cty::c_int;
}

unsafe extern "C" fn sock_write(
    write_context: *mut cty::c_void,
    data: *const cty::c_uchar,
    len: size_t,
) -> cty::c_int {
    //    loop {
    //        asm::nop()
    //    }
    let stream: &mut TcpStream = &mut *(write_context as *mut TcpStream);

    rprintln!("[DBG] sock_write attempting to write {} bytes", len);
    let buf: &[u8] = core::slice::from_raw_parts(data, len as usize);
    stream.write_all(buf).unwrap();

    rprintln!("[DBG] sock_write wrote {} bytes", len);
    len as cty::c_int
}

struct EthContext {
    w5500: *mut cty::c_void,
    spi: *mut cty::c_void,
}

static mut TA0_DN: [u8; 65] = [
    0x30, 0x3F, 0x31, 0x24, 0x30, 0x22, 0x06, 0x03, 0x55, 0x04, 0x0A, 0x13, 0x1B, 0x44, 0x69, 0x67,
    0x69, 0x74, 0x61, 0x6C, 0x20, 0x53, 0x69, 0x67, 0x6E, 0x61, 0x74, 0x75, 0x72, 0x65, 0x20, 0x54,
    0x72, 0x75, 0x73, 0x74, 0x20, 0x43, 0x6F, 0x2E, 0x31, 0x17, 0x30, 0x15, 0x06, 0x03, 0x55, 0x04,
    0x03, 0x13, 0x0E, 0x44, 0x53, 0x54, 0x20, 0x52, 0x6F, 0x6F, 0x74, 0x20, 0x43, 0x41, 0x20, 0x58,
    0x33,
];

// for the LetsEncrypt trust anchor
static mut RSA_N: [u8; 256] = [
    0xDF, 0xAF, 0xE9, 0x97, 0x50, 0x08, 0x83, 0x57, 0xB4, 0xCC, 0x62, 0x65, 0xF6, 0x90, 0x82, 0xEC,
    0xC7, 0xD3, 0x2C, 0x6B, 0x30, 0xCA, 0x5B, 0xEC, 0xD9, 0xC3, 0x7D, 0xC7, 0x40, 0xC1, 0x18, 0x14,
    0x8B, 0xE0, 0xE8, 0x33, 0x76, 0x49, 0x2A, 0xE3, 0x3F, 0x21, 0x49, 0x93, 0xAC, 0x4E, 0x0E, 0xAF,
    0x3E, 0x48, 0xCB, 0x65, 0xEE, 0xFC, 0xD3, 0x21, 0x0F, 0x65, 0xD2, 0x2A, 0xD9, 0x32, 0x8F, 0x8C,
    0xE5, 0xF7, 0x77, 0xB0, 0x12, 0x7B, 0xB5, 0x95, 0xC0, 0x89, 0xA3, 0xA9, 0xBA, 0xED, 0x73, 0x2E,
    0x7A, 0x0C, 0x06, 0x32, 0x83, 0xA2, 0x7E, 0x8A, 0x14, 0x30, 0xCD, 0x11, 0xA0, 0xE1, 0x2A, 0x38,
    0xB9, 0x79, 0x0A, 0x31, 0xFD, 0x50, 0xBD, 0x80, 0x65, 0xDF, 0xB7, 0x51, 0x63, 0x83, 0xC8, 0xE2,
    0x88, 0x61, 0xEA, 0x4B, 0x61, 0x81, 0xEC, 0x52, 0x6B, 0xB9, 0xA2, 0xE2, 0x4B, 0x1A, 0x28, 0x9F,
    0x48, 0xA3, 0x9E, 0x0C, 0xDA, 0x09, 0x8E, 0x3E, 0x17, 0x2E, 0x1E, 0xDD, 0x20, 0xDF, 0x5B, 0xC6,
    0x2A, 0x8A, 0xAB, 0x2E, 0xBD, 0x70, 0xAD, 0xC5, 0x0B, 0x1A, 0x25, 0x90, 0x74, 0x72, 0xC5, 0x7B,
    0x6A, 0xAB, 0x34, 0xD6, 0x30, 0x89, 0xFF, 0xE5, 0x68, 0x13, 0x7B, 0x54, 0x0B, 0xC8, 0xD6, 0xAE,
    0xEC, 0x5A, 0x9C, 0x92, 0x1E, 0x3D, 0x64, 0xB3, 0x8C, 0xC6, 0xDF, 0xBF, 0xC9, 0x41, 0x70, 0xEC,
    0x16, 0x72, 0xD5, 0x26, 0xEC, 0x38, 0x55, 0x39, 0x43, 0xD0, 0xFC, 0xFD, 0x18, 0x5C, 0x40, 0xF1,
    0x97, 0xEB, 0xD5, 0x9A, 0x9B, 0x8D, 0x1D, 0xBA, 0xDA, 0x25, 0xB9, 0xC6, 0xD8, 0xDF, 0xC1, 0x15,
    0x02, 0x3A, 0xAB, 0xDA, 0x6E, 0xF1, 0x3E, 0x2E, 0xF5, 0x5C, 0x08, 0x9C, 0x3C, 0xD6, 0x83, 0x69,
    0xE4, 0x10, 0x9B, 0x19, 0x2A, 0xB6, 0x29, 0x57, 0xE3, 0xE5, 0x3D, 0x9B, 0x9F, 0xF0, 0x02, 0x5D,
];

static mut RSA_E: [u8; 3] = [0x01, 0x00, 0x01];
static mut IO_BUF: [u8; 4096] = [0; 4096];

// NOTE: we want to get real entropy somehow - The entropy below is hardcoded
static ENTROPY: [u8; 64] = [
    0x04, 0xCD, 0x7D, 0x68, 0x64, 0xC6, 0x5E, 0xED, 0x18, 0x7E, 0xA3, 0x51, 0xDC, 0x1E, 0x32, 0x7E,
    0x50, 0xF1, 0xFC, 0x19, 0xE3, 0x99, 0x53, 0x77, 0xC8, 0x06, 0xB0, 0xE3, 0x3B, 0x26, 0xCD, 0x14,
    0xED, 0x2E, 0xB4, 0xDB, 0x24, 0xD5, 0xF0, 0xBC, 0xEF, 0xF0, 0xE7, 0x36, 0xF2, 0x4D, 0x3B, 0xF2,
    0x6C, 0xBA, 0x2C, 0x3A, 0x45, 0xB5, 0x9C, 0xC4, 0x8F, 0xC2, 0xAC, 0x3F, 0x47, 0x63, 0x4C, 0x1E,
];

fn build_trust_anchor() -> br_x509_trust_anchor {
    let dn = br_x500_name {
        data: unsafe { TA0_DN.as_mut_ptr() },
        len: unsafe { TA0_DN.len() as size_t },
    };

    let rsa_key = br_rsa_public_key {
        n: unsafe { RSA_N.as_mut_ptr() },
        nlen: unsafe { RSA_N.len() as size_t },
        e: unsafe { RSA_E.as_mut_ptr() },
        elen: unsafe { RSA_E.len() as size_t },
    };

    let pkey = br_x509_pkey {
        key_type: BR_KEYTYPE_RSA as cty::c_uchar,
        key: br_x509_pkey__bindgen_ty_1 { rsa: rsa_key },
    };

    let ta = br_x509_trust_anchor {
        dn,
        flags: BR_X509_TA_CA, // use for certificates with a root certificate authority
        // flags: 0, // use for self signed certificates
        pkey,
    };

    ta
}

struct SslStream<'a> {
    stream: &'a mut TcpStream<'a>,
    ssl: Ssl,
}

impl<'a> Stream<SslError> for SslStream<'a> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, SslError> {
        rprintln!("[INF] br_sslio_read");
        // panic!("read ssl");

        let rlen = unsafe {
            br_sslio_read(
                &mut self.ssl.ioc as *mut _,
                buf as *mut _ as *mut cty::c_void,
                buf.len(),
            )
        };

        if rlen < 0 {
            rprintln!("[ERR] br_sslio_read failed to read: {}", rlen);
            return Err(SslError::ReadBrErr(self.ssl.cc.eng.err));
        }

        Ok(rlen as usize)
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<(), SslError> {
        rprintln!("[INF] br_sslio_write_all: {} bytes", buf.len());
        let success = unsafe {
            br_sslio_write_all(
                &mut self.ssl.ioc as *mut _,
                buf.as_ptr() as *const _,
                buf.len(),
            )
        };

        // panic!("write ssl");

        if success < 0 {
            rprintln!("[ERR] br_sslio_write_all failed");
            return Err(SslError::WriteBrErr(self.ssl.cc.eng.err));
        }

        rprintln!("[INF] br_sslio_flush");
        unsafe { br_sslio_flush(&mut self.ssl.ioc as *mut _) };
        Ok(())
    }
}

// ************ End of SSL Related ********************

/*
lazy_static! {
    static ref DELAY: Arc<RefCell<Delay>> = {
        let cp: cortex_m::Peripherals = cortex_m::Peripherals::take().unwrap();
        let dp = stm32::Peripherals::take().unwrap();
        let mut rcc = dp.RCC.constrain();
        let mut afio = dp.AFIO.constrain(&mut rcc.apb2);
        let mut flash = dp.FLASH.constrain();
        let clocks = rcc.cfgr.freeze(&mut flash.acr);
        let mut delay = Delay::new(cp.SYST, clocks);
        RefCell::new(delay)
    };
}
*/

fn DelayMs(ms: u16) {
    let cp: cortex_m::Peripherals = cortex_m::Peripherals::take().unwrap();
    let dp = stm32::Peripherals::take().unwrap();
    let mut rcc = dp.RCC.constrain();
    let mut afio = dp.AFIO.constrain(&mut rcc.apb2);
    let mut flash = dp.FLASH.constrain();
    let clocks = rcc.cfgr.freeze(&mut flash.acr);
    let mut delay = Delay::new(cp.SYST, clocks);
    delay.delay_ms(ms)
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

    // DELAY.replace(Some(delay));

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
    rprintln!("[INF] Done initialising");
    // DelayMs(1000);

    let spi = RefCell::new(spi);
    let mut w5500 = W5500::new(cs_ethernet);
    let mut max7219 = MAX7219::new(&mut cs_max7219, 20);
    let mut led_panel = LedPanel::new(&mut max7219, &spi);

    loop {
        let mut stream = TcpStream::new(&mut w5500, Socket::Socket0, &mut delay, &spi);

        match client_connect(&mut led_panel, &mut stream) {
            Ok(()) => rprintln!("[INF] Connection closed"),
            Err(error) => rprintln!("[ERR] {:?}", &error),
        }
    }
}

fn client_connect<'a>(
    led_panel: &mut LedPanel,
    stream: &'a mut TcpStream<'a>,
) -> Result<(), LedDemoError> {
    rprintln!("[INF] Client connecting");

    // remote connection
    // let host_ip = IpAddress::new(51, 140, 68, 75);
    // let host_port = 80;
    // let host = "ninjametal.com";
    // let origin = "http://ninjametal.com";

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

    // ************************************* SSL INIT **************************************************
    // NOTE: I had trouble putting this INIT functionality into its own function because of all the pointers flying around.
    // Even though I moved some data out of the init function the pointers seemed to corrupt themselves. I just dont understand enough yet.

    rprintln!("[INF] Initialising ssl client");
    let host_ssl = "ninjametal.com\0"; // must be null terminated!!

    rprintln!("[INF] building trust anchors");
    let ta = build_trust_anchor();
    let trust_anchors: [br_x509_trust_anchor; 1] = [ta];

    let mut cc = MaybeUninit::<br_ssl_client_context>::uninit();
    let mut x509 = MaybeUninit::<br_x509_minimal_context>::uninit();
    let mut ioc = MaybeUninit::<br_sslio_context>::uninit();

    rprintln!("[INF] br_ssl_client_init_full");
    unsafe {
        br_ssl_client_init_full(
            cc.as_mut_ptr(),
            x509.as_mut_ptr(),
            trust_anchors.as_ptr(),
            trust_anchors.len(),
        )
    };

    let mut cc = unsafe { cc.assume_init() };
    rprintln!("[INF] br_ssl_engine_set_buffer");

    unsafe {
        br_ssl_engine_inject_entropy(
            &mut cc.eng as *mut _,
            (&ENTROPY).as_ptr() as *const cty::c_void,
            ENTROPY.len(),
        )
    };

    unsafe {
        br_ssl_engine_set_buffer(
            &mut cc.eng as *mut _,
            &mut IO_BUF as *mut _ as *mut cty::c_void,
            IO_BUF.len(),
            0, // half duplex
        )
    };

    let context_ptr = stream as *mut _ as *mut cty::c_void;
    rprintln!("[INF] br_ssl_client_reset");

    unsafe {
        br_ssl_client_reset(
            &mut cc as *mut _,
            host_ssl.as_bytes().as_ptr() as *const u8,
            0,
        )
    };

    rprintln!("[INF] br_sslio_init");
    unsafe {
        br_sslio_init(
            ioc.as_mut_ptr(),
            &mut cc.eng as *mut _,
            Some(sock_read),
            context_ptr,
            Some(sock_write),
            context_ptr,
        )
    };

    let ioc = unsafe { ioc.assume_init() };
    let mut ssl = Ssl { cc, ioc };
    let mut ssl_stream = SslStream { stream, ssl };

    // ********************************** END OF SSL INIT **********************************************

    let mut websocket = ws::WebSocketClient::new_client(EmptyRng::new());
    let mut read_buf = [0; 256];
    let mut read_cursor = 0;
    let mut write_buf = [0; 256];
    let mut frame_buf = [0; 256];
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
