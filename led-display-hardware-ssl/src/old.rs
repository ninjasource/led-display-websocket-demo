#![no_std]
#![no_main]
#![allow(unused_imports)]
// #![feature(nll)] // turn this on with nightly to see more lifetime borrow checker detail

mod bearssl;
use bearssl::*;

// hardware
use max7219_dot_matrix::{Command, MAX7219};
use stm32f1xx_hal as hal;
use w5500::{IpAddress, MacAddress, Socket, SocketStatus, W5500};

use core::{fmt::Arguments, mem::MaybeUninit, str::Utf8Error};
use cortex_m::{itm, peripheral::itm::Stim};
use cortex_m_rt::entry;
use cty::size_t;
use embedded_hal::{digital::v2::OutputPin, spi::FullDuplex, spi::Mode, spi::Phase, spi::Polarity};
use embedded_websocket as ws;
use hal::gpio::{
    gpioa::{PA2, PA5, PA6, PA7},
    Alternate, Floating, Input, Output, PushPull,
};
use hal::{delay::Delay, pac::SPI1, prelude::*, spi::Spi, stm32};
use panic_itm;
use ws::{
    EmptyRng, WebSocketClient, WebSocketKey, WebSocketOptions, WebSocketReceiveMessageType,
    WebSocketSendMessageType, WebSocketState,
};

type SpiMapleMini = Spi<
    SPI1,
    (
        PA5<Alternate<PushPull>>,
        PA6<Input<Floating>>,
        PA7<Alternate<PushPull>>,
    ),
>;

type W5500Eth<'a> = W5500<'a, PA2<Output<PushPull>>>;
type WebSocket = WebSocketClient<EmptyRng>;

#[derive(Debug)]
enum SslError {
    WriteBrErr(i32),
    ReadBrErr(i32),
}

#[derive(Debug)]
enum WebServerError {
    Io(stm32f1xx_hal::spi::Error),
    WebSocket(ws::Error),
    Utf8Error,
    Max7219,
    Ssl(SslError),
}

impl From<SslError> for WebServerError {
    fn from(err: SslError) -> WebServerError {
        WebServerError::Ssl(err)
    }
}

impl From<stm32f1xx_hal::spi::Error> for WebServerError {
    fn from(err: stm32f1xx_hal::spi::Error) -> WebServerError {
        WebServerError::Io(err)
    }
}

impl From<ws::Error> for WebServerError {
    fn from(err: ws::Error) -> WebServerError {
        WebServerError::WebSocket(err)
    }
}

impl From<Utf8Error> for WebServerError {
    fn from(_err: Utf8Error) -> WebServerError {
        WebServerError::Utf8Error
    }
}

impl<SpiError, PinError> From<max7219_dot_matrix::Error<SpiError, PinError>> for WebServerError {
    fn from(_err: max7219_dot_matrix::Error<SpiError, PinError>) -> WebServerError {
        // FIXME: capture more of the error than this simple variant
        WebServerError::Max7219
    }
}

struct Connection {
    pub web_socket: WebSocket,
    pub socket: Socket,
    pub socket_status: SocketStatus,
}

impl Connection {
    fn new(socket: Socket) -> Connection {
        Connection {
            web_socket: WebSocketClient::new_client(EmptyRng::new()),
            socket,
            socket_status: SocketStatus::Closed,
        }
    }
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

fn _log_no_new_line(itm: &mut Stim, msg: &str) {
    // FIXME: comment these out when not connected to openocd. itm will crash otherwise
    itm::write_str(itm, msg);
}

fn log(itm: &mut Stim, msg: &str) {
    // FIXME: comment these out when not connected to openocd. itm will crash otherwise
    itm::write_str(itm, msg);
    itm::write_str(itm, "\n");
}

fn log_fmt(itm: &mut Stim, args: Arguments) {
    // FIXME: comment these out when not connected to openocd. itm will crash otherwise
    itm::write_fmt(itm, args);
    itm::write_str(itm, "\n");
}

unsafe extern "C" fn sock_read(
    read_context: *mut cty::c_void,
    data: *mut cty::c_uchar,
    len: size_t,
) -> cty::c_int {
    let context: &mut EthContext = &mut *(read_context as *mut EthContext);
    let itm: &mut Stim = &mut *(context.itm as *mut Stim);
    let buf: &mut [u8] = core::slice::from_raw_parts_mut(data, len as usize);
    let spi: &mut SpiMapleMini = &mut *(context.spi as *mut SpiMapleMini);
    let w5500: &mut W5500Eth = &mut *(context.w5500 as *mut W5500Eth);

    let max_len = if buf.len() > len as usize {
        len as usize
    } else {
        buf.len()
    };

    let size = w5500
        .try_receive_tcp(spi, Socket::Socket0, &mut buf[..max_len])
        .unwrap();
    if let Some(size) = size {
        log_fmt(itm, format_args!("[DBG] sock_read received {} bytes", size));
        return size as cty::c_int;
    }

    0
}

unsafe extern "C" fn sock_write(
    write_context: *mut cty::c_void,
    data: *const cty::c_uchar,
    len: size_t,
) -> cty::c_int {
    let context: &mut EthContext = &mut *(write_context as *mut EthContext);
    let itm: &mut Stim = &mut *(context.itm as *mut Stim);

    log_fmt(
        itm,
        format_args!("[DBG] sock_write attempting to write {} bytes", len),
    );

    let buf: &[u8] = core::slice::from_raw_parts(data, len as usize);
    let spi: &mut SpiMapleMini = &mut *(context.spi as *mut SpiMapleMini);
    let w5500: &mut W5500Eth = &mut *(context.w5500 as *mut W5500Eth);

    eth_write(spi, Socket::Socket0, w5500, buf).unwrap();
    log_fmt(itm, format_args!("[DBG] sock_write wrote {} bytes", len));
    len as cty::c_int
}

struct EthContext {
    w5500: *mut cty::c_void,
    spi: *mut cty::c_void,
    itm: *mut cty::c_void,
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

#[entry]
fn main() -> ! {
    let mut cp: cortex_m::Peripherals = cortex_m::Peripherals::take().unwrap();
    let dp = stm32::Peripherals::take().unwrap();
    let itm = &mut cp.ITM;

    log(&mut itm.stim[0], "[INF] Initializing");

    let mut rcc = dp.RCC.constrain();
    let mut afio = dp.AFIO.constrain(&mut rcc.apb2);
    let mut flash = dp.FLASH.constrain();
    let clocks = rcc.cfgr.freeze(&mut flash.acr);
    let mut delay = Delay::new(cp.SYST, clocks);
    let mut gpioa = dp.GPIOA.split(&mut rcc.apb2);

    let sck = gpioa.pa5.into_alternate_push_pull(&mut gpioa.crl);
    let miso = gpioa.pa6;
    let mosi = gpioa.pa7.into_alternate_push_pull(&mut gpioa.crl);
    //let mut cs_max7219 = gpioa.pa4.into_push_pull_output(&mut gpioa.crl);
    let mut cs_ethernet = gpioa.pa2.into_push_pull_output(&mut gpioa.crl);

    let mut spi: SpiMapleMini = Spi::spi1(
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
    // let mut max7219 = MAX7219::new(&mut cs_max7219, 20);
    let mut w5500: W5500Eth = W5500::new(&mut cs_ethernet);

    run_loop(&mut spi, &mut itm.stim[0], &mut w5500)
        .map_err(|e| {
            log_fmt(
                &mut itm.stim[0],
                format_args!("[ERR] Unexpected error: {:?}", e),
            )
        })
        .unwrap();

    loop {}
}

fn run_loop(
    spi: &mut SpiMapleMini,
    itm: &mut Stim,
    w5500: &mut W5500Eth,
) -> Result<(), WebServerError> {
    log(itm, "[INF] Done initializing");

    loop {
        client_connect(spi, itm, w5500)
            .map_err(|e| log_fmt(itm, format_args!("[ERR] Unexpected error: {:?}", e)))
            .unwrap();
    }
}

struct Ssl {
    cc: br_ssl_client_context,
    ioc: br_sslio_context,
}

fn client_connect(
    spi: &mut SpiMapleMini,
    itm: &mut Stim,
    w5500: &mut W5500Eth,
) -> Result<(), WebServerError> {
    log(itm, "[INF] Running client_connect");
    w5500.set_mode(spi, false, false, false, false)?;
    w5500.set_mac(spi, &MacAddress::new(0x02, 0x01, 0x02, 0x03, 0x04, 0x05))?;
    w5500.set_subnet(spi, &IpAddress::new(255, 255, 255, 0))?;

    // set this when internet sharing is on
    // w5500.set_ip(spi, &IpAddress::new(192, 168, 137, 33))?;
    // w5500.set_gateway(spi, &IpAddress::new(192, 168, 137, 1))?;

    // set this when connected directly to the internet
    w5500.set_ip(spi, &IpAddress::new(192, 168, 1, 33))?;
    w5500.set_gateway(spi, &IpAddress::new(192, 168, 1, 1))?;

    // make sure the connection is closed before we start
    let mut connection = Connection::new(Socket::Socket0);
    w5500.set_protocol(spi, connection.socket, w5500::Protocol::TCP)?;
    w5500.dissconnect(spi, connection.socket)?;

    //let mut buffer: [u8; 3000] = [0; 3000];
    let mut buffer: [u8; 512] = [0; 512];
    let mut ws_buffer: [u8; 256] = [0; 256];

    // connecting to the internet
    let host_ip = IpAddress::new(51, 140, 68, 75);
    let host_port = 443;
    //let host_port = 80;
    let host = "ninjametal.com";
    let origin = "https://ninjametal.com";

    /*
        // connecting to a local server
        let host_ip = IpAddress::new(192, 168, 1, 152);
        let host_port = 443;
        let host = "192.168.1.152";
        let origin = "https://192.168.1.152";
    */
    // open
    log_fmt(
        itm,
        format_args!(
            "[INF] TCP Opening {}:{} on {:?}",
            &host_ip, host_port, connection.socket
        ),
    );
    let mut web_socket = ws::WebSocketClient::new_client(EmptyRng::new());
    w5500.open_tcp(spi, connection.socket)?;
    w5500.connect(spi, connection.socket, &host_ip, host_port)?;

    log(itm, "[INF] Initialising ssl client");
    let host_ssl = "ninjametal.com\0"; // must be null terminated!!

    // ************************************* SSL INIT **************************************************
    // NOTE: I had trouble putting this INIT functionality into its own function because of all the pointers flying around.
    // Even though I moved some data out of the init function the pointers seemed to corrupt themselves. I just dont understand enough yet.

    log(itm, "[INF] building trust anchors");
    let ta = build_trust_anchor();
    let trust_anchors: [br_x509_trust_anchor; 1] = [ta];

    let mut cc = MaybeUninit::<br_ssl_client_context>::uninit();
    let mut x509 = MaybeUninit::<br_x509_minimal_context>::uninit();
    let mut ioc = MaybeUninit::<br_sslio_context>::uninit();

    log(itm, "[INF] br_ssl_client_init_full");
    unsafe {
        br_ssl_client_init_full(
            cc.as_mut_ptr(),
            x509.as_mut_ptr(),
            trust_anchors.as_ptr(),
            trust_anchors.len(),
        )
    };

    let mut cc = unsafe { cc.assume_init() };
    log(itm, "[INF] br_ssl_engine_set_buffer");

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

    let w5500_ptr = w5500 as *mut _ as *mut cty::c_void;
    let spi_ptr = spi as *mut _ as *mut cty::c_void;
    let itm_ptr = itm as *mut _ as *mut cty::c_void;

    let mut context = EthContext {
        w5500: w5500_ptr,
        spi: spi_ptr,
        itm: itm_ptr,
    };

    let context_ptr = &mut context as *mut _ as *mut cty::c_void;
    log(itm, "[INF] br_ssl_client_reset");

    unsafe {
        br_ssl_client_reset(
            &mut cc as *mut _,
            host_ssl.as_bytes().as_ptr() as *const u8,
            0,
        )
    };

    log(itm, "[INF] br_sslio_init");
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

    // ********************************** END OF SSL INIT **********************************************

    log(itm, "[INF] Done initialising ssl client");

    loop {
        match w5500.get_socket_status(spi, connection.socket) {
            Ok(Some(socket_status)) => {
                if connection.socket_status != socket_status {
                    // print status change
                    log_fmt(
                        itm,
                        format_args!(
                            "[INF] Socket status: {:?} -> {:?}",
                            connection.socket_status, socket_status
                        ),
                    );
                    connection.socket_status = socket_status;
                }
                match socket_status {
                    SocketStatus::CloseWait | SocketStatus::Closed => {
                        log(itm, "Attempting to reconnect");
                        web_socket = ws::WebSocketClient::new_client(EmptyRng::new());
                        w5500.open_tcp(spi, connection.socket)?;
                        w5500.connect(spi, Socket::Socket0, &host_ip, host_port)?;
                    }
                    SocketStatus::Established => {
                        if web_socket.state == WebSocketState::None {
                            // initiate a websocket opening handshake
                            let websocket_options = WebSocketOptions {
                                path: "/ws/ledpanel",
                                host,
                                origin,
                                sub_protocols: None,
                                additional_headers: None,
                            };
                            let (len, _web_socket_key) =
                                web_socket.client_connect(&websocket_options, &mut ws_buffer)?;
                            log(itm, "[INF] Sending opening websocket handshake");

                            ssl_write(&mut ssl, &ws_buffer[..len], itm)?;
                            log(itm, "[INF] websocket handshake sent");
                        }

                        ssl_read_client(
                            &mut ssl,
                            spi,
                            Socket::Socket0,
                            &mut web_socket,
                            &mut buffer,
                            &mut ws_buffer,
                            itm,
                            w5500,
                        )?;
                    }
                    _ => {} // do nothing
                }
            }
            Ok(None) => {
                log(itm, "[ERR] Unknown socket status");
                return Ok(());
            }
            Err(e) => log_fmt(
                itm,
                format_args!("[ERR] Cannot read socket status: {:?}", e),
            ),
        }
    }
}

fn ssl_write(ssl: &mut Ssl, buffer: &[u8], itm: &mut Stim) -> Result<(), WebServerError> {
    let success = unsafe {
        br_sslio_write_all(
            &mut ssl.ioc as *mut _,
            buffer.as_ptr() as *const _,
            buffer.len(),
        )
    };

    if success < 0 {
        log(itm, "[ERR] br_sslio_write_all failed");
        return Err(WebServerError::Ssl(SslError::WriteBrErr(ssl.cc.eng.err)));
    }

    log(itm, "[INF] br_sslio_flush");
    unsafe { br_sslio_flush(&mut ssl.ioc as *mut _) };
    Ok(())
}

fn ws_write_back(
    ssl: &mut Ssl,
    web_socket: &mut WebSocket,
    eth_buffer: &mut [u8],
    ws_buffer: &mut [u8],
    count: usize,
    send_message_type: WebSocketSendMessageType,
    itm: &mut Stim,
) -> Result<(), WebServerError> {
    eth_buffer[..count].copy_from_slice(&ws_buffer[..count]);
    let ws_to_send = web_socket.write(send_message_type, true, &eth_buffer[..count], ws_buffer)?;
    ssl_write(ssl, &ws_buffer[..ws_to_send], itm)?;
    log_fmt(
        itm,
        format_args!(
            "[DBG] Websocket encoded {:#?}: {} bytes",
            send_message_type, ws_to_send
        ),
    );
    Ok(())
}

fn ws_read(
    ssl_client: &mut Ssl,
    spi: &mut SpiMapleMini,
    socket: Socket,
    web_socket: &mut WebSocket,
    eth_buffer: &mut [u8],
    ws_buffer: &mut [u8],
    size: usize,
    itm: &mut Stim,
    w5500: &mut W5500Eth,
) -> Result<(), WebServerError> {
    let ws_read_result = web_socket.read(&eth_buffer[..size], ws_buffer)?;
    log_fmt(
        itm,
        format_args!(
            "[DBG] Websocket decoded {:#?}: {} bytes",
            ws_read_result.message_type, ws_read_result.len_to
        ),
    );
    match ws_read_result.message_type {
        WebSocketReceiveMessageType::Text => {
            let message = ::core::str::from_utf8(&ws_buffer[..ws_read_result.len_to])?;
            let print_msg = if message.len() > 100 {
                &message[..100] // limit what we log
            } else {
                &message
            };
            log_fmt(itm, format_args!("[INF] Websocket: {}", print_msg));
            // scroll_str(itm, max7219, spi, message)?;
        }
        WebSocketReceiveMessageType::Binary => {
            // do nothing
            log(itm, "[WRN] Binary message ignored");
        }
        WebSocketReceiveMessageType::CloseMustReply => {
            let close_status = ws_read_result.close_status.unwrap(); // this should never fail
            {
                if ws_read_result.len_to > 2 {
                    let message = ::core::str::from_utf8(&ws_buffer[2..ws_read_result.len_to])?;
                    log_fmt(
                        itm,
                        format_args!(
                            "[INF] Websocket close status {:#?}: {}",
                            close_status, message
                        ),
                    );
                } else {
                    log_fmt(
                        itm,
                        format_args!("[INF] Websocket close status {:#?}", close_status),
                    );
                }
            }

            ws_write_back(
                ssl_client,
                web_socket,
                eth_buffer,
                ws_buffer,
                ws_read_result.len_to,
                WebSocketSendMessageType::CloseReply,
                itm,
            )?;
            w5500.close(spi, socket)?;
            log(itm, "[INF] TCP connection closed");
        }
        WebSocketReceiveMessageType::Ping => {
            ws_write_back(
                ssl_client,
                web_socket,
                eth_buffer,
                ws_buffer,
                ws_read_result.len_to,
                WebSocketSendMessageType::Pong,
                itm,
            )?;
        }
        WebSocketReceiveMessageType::Pong => {
            // do nothing
        }
        WebSocketReceiveMessageType::CloseCompleted => {
            log(itm, "[INF] Websocket close handshake completed");
            w5500.close(spi, socket)?;
            log(itm, "[INF] TCP connection closed");
        }
    }

    Ok(())
}

fn eth_write(
    spi: &mut SpiMapleMini,
    socket: Socket,
    w5500: &mut W5500Eth,
    buffer: &[u8],
) -> Result<(), WebServerError> {
    let mut start = 0;
    loop {
        let bytes_sent = w5500.send_tcp(spi, socket, &buffer[start..])?;
        start += bytes_sent;
        if start == buffer.len() {
            return Ok(());
        }
    }
}

fn ssl_read(
    ssl: &mut Ssl,
    into_buffer: &mut [u8],
    itm: &mut Stim,
) -> Result<Option<usize>, WebServerError> {
    log(itm, "[INF] br_sslio_read");

    let rlen = unsafe {
        br_sslio_read(
            &mut ssl.ioc as *mut _,
            into_buffer as *mut _ as *mut cty::c_void,
            into_buffer.len(),
        )
    };

    if rlen < 0 {
        log_fmt(
            itm,
            format_args!("[ERR] br_sslio_read failed to read: {}", rlen),
        );

        return Err(WebServerError::Ssl(SslError::ReadBrErr(ssl.cc.eng.err)));
    }

    Ok(Some(rlen as usize))
}

fn ssl_read_client(
    ssl: &mut Ssl,
    spi: &mut SpiMapleMini,
    socket: Socket,
    web_socket: &mut WebSocket,
    eth_buffer: &mut [u8],
    ws_buffer: &mut [u8],
    itm: &mut Stim,
    w5500: &mut W5500Eth,
) -> Result<(), WebServerError> {
    let size = ssl_read(ssl, eth_buffer, itm)?;
    if let Some(size) = size {
        log_fmt(itm, format_args!("[DBG] Ethernet received {} bytes", size));

        match web_socket.state {
            WebSocketState::Connecting => {
                log(itm, "[INF] Reading opening websocket handshake response");
                let sec_websocket_key = WebSocketKey::new();
                web_socket.client_accept(&sec_websocket_key, &eth_buffer[..size])?;
                log(itm, "[INF] Websocket opening handshake complete");
            }
            WebSocketState::Open => {
                ws_read(
                    ssl, spi, socket, web_socket, eth_buffer, ws_buffer, size, itm, w5500,
                )?;
            }
            state => log_fmt(
                itm,
                format_args!("[ERR] Unexpected WebSocketState: {:#?}", state),
            ),
        };
    };

    Ok(())
}
