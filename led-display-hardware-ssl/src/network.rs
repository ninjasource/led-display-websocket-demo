use crate::bearssl::*;
use crate::{SpiError, SpiPhysical, SpiTransfer};
use core::{cell::RefCell, convert::Infallible};
use cortex_m::prelude::_embedded_hal_blocking_delay_DelayMs;
use cty::size_t;
use embedded_websocket::framer::Stream;
use stm32f1xx_hal::{
    delay::Delay,
    gpio::{gpioa::PA2, Output, PushPull},
};
use w5500::{BufferSize, IpAddress, MacAddress, Socket, SocketStatus, W5500};

// ************ SSL Related ********************

static mut TA0_DN: [u8; 81] = [
    0x30, 0x4F, 0x31, 0x0B, 0x30, 0x09, 0x06, 0x03, 0x55, 0x04, 0x06, 0x13, 0x02, 0x55, 0x53, 0x31,
    0x29, 0x30, 0x27, 0x06, 0x03, 0x55, 0x04, 0x0A, 0x13, 0x20, 0x49, 0x6E, 0x74, 0x65, 0x72, 0x6E,
    0x65, 0x74, 0x20, 0x53, 0x65, 0x63, 0x75, 0x72, 0x69, 0x74, 0x79, 0x20, 0x52, 0x65, 0x73, 0x65,
    0x61, 0x72, 0x63, 0x68, 0x20, 0x47, 0x72, 0x6F, 0x75, 0x70, 0x31, 0x15, 0x30, 0x13, 0x06, 0x03,
    0x55, 0x04, 0x03, 0x13, 0x0C, 0x49, 0x53, 0x52, 0x47, 0x20, 0x52, 0x6F, 0x6F, 0x74, 0x20, 0x58,
    0x31,
];

// for the LetsEncrypt trust anchor
static mut TA0_RSA_N: [u8; 512] = [
    0xAD, 0xE8, 0x24, 0x73, 0xF4, 0x14, 0x37, 0xF3, 0x9B, 0x9E, 0x2B, 0x57, 0x28, 0x1C, 0x87, 0xBE,
    0xDC, 0xB7, 0xDF, 0x38, 0x90, 0x8C, 0x6E, 0x3C, 0xE6, 0x57, 0xA0, 0x78, 0xF7, 0x75, 0xC2, 0xA2,
    0xFE, 0xF5, 0x6A, 0x6E, 0xF6, 0x00, 0x4F, 0x28, 0xDB, 0xDE, 0x68, 0x86, 0x6C, 0x44, 0x93, 0xB6,
    0xB1, 0x63, 0xFD, 0x14, 0x12, 0x6B, 0xBF, 0x1F, 0xD2, 0xEA, 0x31, 0x9B, 0x21, 0x7E, 0xD1, 0x33,
    0x3C, 0xBA, 0x48, 0xF5, 0xDD, 0x79, 0xDF, 0xB3, 0xB8, 0xFF, 0x12, 0xF1, 0x21, 0x9A, 0x4B, 0xC1,
    0x8A, 0x86, 0x71, 0x69, 0x4A, 0x66, 0x66, 0x6C, 0x8F, 0x7E, 0x3C, 0x70, 0xBF, 0xAD, 0x29, 0x22,
    0x06, 0xF3, 0xE4, 0xC0, 0xE6, 0x80, 0xAE, 0xE2, 0x4B, 0x8F, 0xB7, 0x99, 0x7E, 0x94, 0x03, 0x9F,
    0xD3, 0x47, 0x97, 0x7C, 0x99, 0x48, 0x23, 0x53, 0xE8, 0x38, 0xAE, 0x4F, 0x0A, 0x6F, 0x83, 0x2E,
    0xD1, 0x49, 0x57, 0x8C, 0x80, 0x74, 0xB6, 0xDA, 0x2F, 0xD0, 0x38, 0x8D, 0x7B, 0x03, 0x70, 0x21,
    0x1B, 0x75, 0xF2, 0x30, 0x3C, 0xFA, 0x8F, 0xAE, 0xDD, 0xDA, 0x63, 0xAB, 0xEB, 0x16, 0x4F, 0xC2,
    0x8E, 0x11, 0x4B, 0x7E, 0xCF, 0x0B, 0xE8, 0xFF, 0xB5, 0x77, 0x2E, 0xF4, 0xB2, 0x7B, 0x4A, 0xE0,
    0x4C, 0x12, 0x25, 0x0C, 0x70, 0x8D, 0x03, 0x29, 0xA0, 0xE1, 0x53, 0x24, 0xEC, 0x13, 0xD9, 0xEE,
    0x19, 0xBF, 0x10, 0xB3, 0x4A, 0x8C, 0x3F, 0x89, 0xA3, 0x61, 0x51, 0xDE, 0xAC, 0x87, 0x07, 0x94,
    0xF4, 0x63, 0x71, 0xEC, 0x2E, 0xE2, 0x6F, 0x5B, 0x98, 0x81, 0xE1, 0x89, 0x5C, 0x34, 0x79, 0x6C,
    0x76, 0xEF, 0x3B, 0x90, 0x62, 0x79, 0xE6, 0xDB, 0xA4, 0x9A, 0x2F, 0x26, 0xC5, 0xD0, 0x10, 0xE1,
    0x0E, 0xDE, 0xD9, 0x10, 0x8E, 0x16, 0xFB, 0xB7, 0xF7, 0xA8, 0xF7, 0xC7, 0xE5, 0x02, 0x07, 0x98,
    0x8F, 0x36, 0x08, 0x95, 0xE7, 0xE2, 0x37, 0x96, 0x0D, 0x36, 0x75, 0x9E, 0xFB, 0x0E, 0x72, 0xB1,
    0x1D, 0x9B, 0xBC, 0x03, 0xF9, 0x49, 0x05, 0xD8, 0x81, 0xDD, 0x05, 0xB4, 0x2A, 0xD6, 0x41, 0xE9,
    0xAC, 0x01, 0x76, 0x95, 0x0A, 0x0F, 0xD8, 0xDF, 0xD5, 0xBD, 0x12, 0x1F, 0x35, 0x2F, 0x28, 0x17,
    0x6C, 0xD2, 0x98, 0xC1, 0xA8, 0x09, 0x64, 0x77, 0x6E, 0x47, 0x37, 0xBA, 0xCE, 0xAC, 0x59, 0x5E,
    0x68, 0x9D, 0x7F, 0x72, 0xD6, 0x89, 0xC5, 0x06, 0x41, 0x29, 0x3E, 0x59, 0x3E, 0xDD, 0x26, 0xF5,
    0x24, 0xC9, 0x11, 0xA7, 0x5A, 0xA3, 0x4C, 0x40, 0x1F, 0x46, 0xA1, 0x99, 0xB5, 0xA7, 0x3A, 0x51,
    0x6E, 0x86, 0x3B, 0x9E, 0x7D, 0x72, 0xA7, 0x12, 0x05, 0x78, 0x59, 0xED, 0x3E, 0x51, 0x78, 0x15,
    0x0B, 0x03, 0x8F, 0x8D, 0xD0, 0x2F, 0x05, 0xB2, 0x3E, 0x7B, 0x4A, 0x1C, 0x4B, 0x73, 0x05, 0x12,
    0xFC, 0xC6, 0xEA, 0xE0, 0x50, 0x13, 0x7C, 0x43, 0x93, 0x74, 0xB3, 0xCA, 0x74, 0xE7, 0x8E, 0x1F,
    0x01, 0x08, 0xD0, 0x30, 0xD4, 0x5B, 0x71, 0x36, 0xB4, 0x07, 0xBA, 0xC1, 0x30, 0x30, 0x5C, 0x48,
    0xB7, 0x82, 0x3B, 0x98, 0xA6, 0x7D, 0x60, 0x8A, 0xA2, 0xA3, 0x29, 0x82, 0xCC, 0xBA, 0xBD, 0x83,
    0x04, 0x1B, 0xA2, 0x83, 0x03, 0x41, 0xA1, 0xD6, 0x05, 0xF1, 0x1B, 0xC2, 0xB6, 0xF0, 0xA8, 0x7C,
    0x86, 0x3B, 0x46, 0xA8, 0x48, 0x2A, 0x88, 0xDC, 0x76, 0x9A, 0x76, 0xBF, 0x1F, 0x6A, 0xA5, 0x3D,
    0x19, 0x8F, 0xEB, 0x38, 0xF3, 0x64, 0xDE, 0xC8, 0x2B, 0x0D, 0x0A, 0x28, 0xFF, 0xF7, 0xDB, 0xE2,
    0x15, 0x42, 0xD4, 0x22, 0xD0, 0x27, 0x5D, 0xE1, 0x79, 0xFE, 0x18, 0xE7, 0x70, 0x88, 0xAD, 0x4E,
    0xE6, 0xD9, 0x8B, 0x3A, 0xC6, 0xDD, 0x27, 0x51, 0x6E, 0xFF, 0xBC, 0x64, 0xF5, 0x33, 0x43, 0x4F,
];

pub static mut TA0_RSA_E: [u8; 3] = [0x01, 0x00, 0x01];
pub static mut IO_BUF: [u8; 4096] = [0; 4096];
// pub static mut IO_BUF: [u8; 2048] = [0; 2048];
pub static mut READ_BUF: [u8; 512] = [0; 512];
pub static mut WRITE_BUF: [u8; 512] = [0; 512];
pub static mut FRAME_BUF: [u8; 128] = [0; 128];
pub const NETWORK_HOST: &[u8; 15usize] = b"ninjametal.com\0"; // must be null terminated!!

// NOTE: we want to get real entropy somehow - The entropy below is hardcoded
pub static ENTROPY: [u8; 64] = [
    0x04, 0xCD, 0x7D, 0x68, 0x64, 0xC6, 0x5E, 0xED, 0x18, 0x7E, 0xA3, 0x51, 0xDC, 0x1E, 0x32, 0x7E,
    0x50, 0xF1, 0xFC, 0x19, 0xE3, 0x99, 0x53, 0x77, 0xC8, 0x06, 0xB0, 0xE3, 0x3B, 0x26, 0xCD, 0x14,
    0xED, 0x2E, 0xB4, 0xDB, 0x24, 0xD5, 0xF0, 0xBC, 0xEF, 0xF0, 0xE7, 0x36, 0xF2, 0x4D, 0x3B, 0xF2,
    0x6C, 0xBA, 0x2C, 0x3A, 0x45, 0xB5, 0x9C, 0xC4, 0x8F, 0xC2, 0xAC, 0x3F, 0x47, 0x63, 0x4C, 0x1E,
];

pub fn build_trust_anchor() -> br_x509_trust_anchor {
    let dn = br_x500_name {
        data: unsafe { TA0_DN.as_mut_ptr() },
        len: unsafe { TA0_DN.len() as size_t },
    };

    let rsa_key = br_rsa_public_key {
        n: unsafe { TA0_RSA_N.as_mut_ptr() },
        nlen: unsafe { TA0_RSA_N.len() as size_t },
        e: unsafe { TA0_RSA_E.as_mut_ptr() },
        elen: unsafe { TA0_RSA_E.len() as size_t },
    };

    let pkey = br_x509_pkey {
        key_type: BR_KEYTYPE_RSA as cty::c_uchar,
        key: br_x509_pkey__bindgen_ty_1 { rsa: rsa_key },
    };

    br_x509_trust_anchor {
        dn,
        flags: BR_X509_TA_CA, // use for certificates with a root certificate authority
        // flags: 0, // use for self signed certificates
        pkey,
    }
}

// no mangle so that the linker can find this function which will be called from BearSSL
#[no_mangle]
extern "C" fn time(_time: &crate::bearssl::__time_t) -> crate::bearssl::__time_t {
    rprintln!("[INF] time");
    1622375903
}

// since we are not linking the clib we need to implement this function ourselves
#[no_mangle]
extern "C" fn strlen(s: *const cty::c_char) -> isize {
    let mut count = 0;
    while (unsafe { *s.add(count) } != b'\0') {
        count += 1;
    }

    rprintln!("[INF] strlen: {}", count);
    count as isize
}

pub extern "C" fn sock_read(
    read_context: *mut cty::c_void,
    data: *mut cty::c_uchar,
    len: size_t,
) -> cty::c_int {
    rprintln!("[INF] sock_read");
    let context: &mut EthContext = unsafe { &mut *(read_context as *mut EthContext) };
    let spi = unsafe { &*(context.spi as *const RefCell<SpiPhysical>) };
    let spi = &mut *spi.borrow_mut();
    let w5500 = unsafe { &mut *(context.w5500 as *mut W5500Physical) };
    let connection = unsafe { &mut *(context.connection as *mut Connection) };
    let delay = unsafe { &*(context.delay as *const RefCell<Delay>) };
    let delay = &mut *delay.borrow_mut();
    let client_context = unsafe { &mut *context.client_context };

    let buf: &mut [u8] = unsafe { core::slice::from_raw_parts_mut(data, len as usize) };
    rprintln!("[INF] sock_read into buffer of {} bytes", len);

    let mut total_read = 0;

    loop {
        match wait_for_is_connected(w5500, spi, connection, delay) {
            Ok(()) => match w5500.try_receive_tcp(spi, connection.socket, &mut buf[total_read..]) {
                Ok(Some(len_read)) => {
                    rprintln!(
                        "[INF] sock_read received {} bytes. BearSslErr: {}",
                        len_read,
                        client_context.eng.err
                    );
                    total_read += len_read;
                    if total_read == len {
                        return len as cty::c_int;
                    }
                }
                Ok(None) => {
                    rprintln!("[INF] sock_read waiting to receive bytes");
                }
                Err(e) => rprintln!("[ERR] sock_read try_receive_tcp Err: {:?}", e),
            },
            Err(e) => rprintln!("[ERR] sock_read waiting for is connected Err: {:?}", e),
        }

        delay.delay_ms(50_u16);
    }
}

pub extern "C" fn sock_write(
    write_context: *mut cty::c_void,
    data: *const cty::c_uchar,
    len: size_t,
) -> cty::c_int {
    rprintln!("[INF] sock_write");

    let buf: &[u8] = unsafe { core::slice::from_raw_parts(data, len as usize) };
    rprintln!("[INF] sock_write: Sending {} bytes", buf.len());

    let context: &mut EthContext = unsafe { &mut *(write_context as *mut EthContext) };
    let spi = unsafe { &*context.spi };
    let spi = &mut *spi.borrow_mut();
    let w5500 = unsafe { &mut *context.w5500 };
    let connection = unsafe { &mut *context.connection };
    let delay = unsafe { &*context.delay };
    let delay = &mut *delay.borrow_mut();
    let client_context = unsafe { &mut *context.client_context };
    let mut start = 0;

    loop {
        match wait_for_is_connected(w5500, spi, connection, delay) {
            Ok(()) => match w5500.send_tcp(spi, connection.socket, &buf[start..]) {
                Ok(bytes_sent) => {
                    start += bytes_sent;
                    rprintln!(
                        "[INF] sock_write: Sent {} bytes. BearSslErr: {}",
                        bytes_sent,
                        client_context.eng.err
                    );

                    if start == buf.len() {
                        return len as cty::c_int;
                    }
                }
                Err(e) => rprintln!(
                    "[ERR] sock_write send_tcp Err: {:?}, BearSslErr: {}",
                    e,
                    client_context.eng.err
                ),
            },
            Err(e) => rprintln!(
                "[ERR] sock_write waiting for is connected Err: {:?}, BearSslErr: {}",
                e,
                client_context.eng.err
            ),
        }

        delay.delay_ms(50_u16);
    }
}

// #[derive(Debug)]
// enum SslError {
//    WriteBrErr(i32),
//    ReadBrErr(i32),
// }

impl<'a> Stream<NetworkError> for SslStream<'a> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, NetworkError> {
        rprintln!("[INF] read");

        let rlen = unsafe {
            br_sslio_read(
                self.io_context,
                buf as *mut _ as *mut cty::c_void,
                buf.len(),
            )
        };

        if rlen < 0 {
            rprintln!("[ERR] br_sslio_read failed to read. rlen: {}", rlen);

            // return Err(SslError::ReadBrErr(self.ssl.cc.eng.err));
            return Err(NetworkError::Closed);
        }

        Ok(rlen as usize)
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<(), NetworkError> {
        rprintln!("[INF] write_all: {} bytes", buf.len());

        let success =
            unsafe { br_sslio_write_all(self.io_context, buf.as_ptr() as *const _, buf.len()) };

        if success < 0 {
            rprintln!("[ERR] br_sslio_write_all failed: {}", success);

            // return Err(SslError::WriteBrErr(self.ssl.cc.eng.err));
            return Err(NetworkError::Closed);
        }

        rprintln!("[INF] br_sslio_flush");
        unsafe { br_sslio_flush(self.io_context) };
        Ok(())
    }
}

// ************ End of SSL Related ********************

#[derive(Debug)]
pub enum NetworkError {
    Io(W5500Error),
    Closed,
    SocketStatusNone,
}

impl From<W5500Error> for NetworkError {
    fn from(err: W5500Error) -> NetworkError {
        NetworkError::Io(err)
    }
}

pub struct Connection {
    pub socket: Socket,
    pub socket_status: SocketStatus,
}

impl Connection {
    pub fn new(socket: Socket) -> Connection {
        Connection {
            socket,
            socket_status: SocketStatus::Closed,
        }
    }
}

// W5500 ethernet card with CS pin PA2
type W5500Physical = W5500<PA2<Output<PushPull>>>;

// the CS output pin on stm32f1xx_hal is Infallible
type W5500Error = w5500::Error<SpiError, Infallible>;

pub struct EthContext {
    pub spi: *const RefCell<SpiPhysical>,
    pub connection: *mut Connection,
    pub delay: *const RefCell<Delay>,
    pub w5500: *mut W5500Physical,
    pub client_context: *mut br_ssl_client_context,
}

pub struct SslStream<'a> {
    w5500: &'a mut W5500Physical,
    connection: &'a mut Connection,
    spi: &'a RefCell<SpiTransfer>,
    delay: &'a RefCell<Delay>,
    io_context: *mut br_sslio_context,
}

impl<'a> SslStream<'a> {
    pub fn new(
        w5500: &'a mut W5500Physical,
        connection: &'a mut Connection,
        spi: &'a RefCell<SpiTransfer>,
        delay: &'a RefCell<Delay>,
        io_context: *mut br_sslio_context,
    ) -> Self {
        Self {
            w5500,
            connection,
            spi,
            delay,
            io_context,
        }
    }

    pub fn connect(&mut self, host_ip: &IpAddress, host_port: u16) -> Result<(), NetworkError> {
        rprintln!("[INF] Connecting to {}:{}", host_ip, host_port);
        let w5500 = &mut self.w5500;
        let spi = &mut *self.spi.borrow_mut();
        let delay = &mut *self.delay.borrow_mut();

        w5500.set_mode(spi, false, false, false, false)?;
        w5500.set_mac(spi, &MacAddress::new(0x02, 0x01, 0x02, 0x03, 0x04, 0x05))?;
        w5500.set_subnet(spi, &IpAddress::new(255, 255, 255, 0))?;
        w5500.set_ip(spi, &IpAddress::new(192, 168, 1, 33))?;
        w5500.set_gateway(spi, &IpAddress::new(192, 168, 1, 1))?;

        // since we are only using one socket we might as well use all the memory available for sockets
        w5500.set_rx_buffer_size(spi, self.connection.socket, BufferSize::Size16KB)?;
        w5500.set_tx_buffer_size(spi, self.connection.socket, BufferSize::Size16KB)?;
        w5500.set_protocol(spi, self.connection.socket, w5500::Protocol::TCP)?;
        w5500.dissconnect(spi, self.connection.socket)?;
        w5500.open_tcp(spi, self.connection.socket)?;
        w5500.connect(spi, self.connection.socket, host_ip, host_port)?;

        wait_for_is_connected(w5500, spi, self.connection, delay)?;
        rprintln!("[INF] Client connected");
        Ok(())
    }
}

pub fn wait_for_is_connected(
    w5500: &mut W5500Physical,
    spi: &mut SpiTransfer,
    connection: &mut Connection,
    delay: &mut Delay,
) -> Result<(), NetworkError> {
    loop {
        match w5500.get_socket_status(spi, connection.socket)? {
            Some(status) => {
                if status != connection.socket_status {
                    rprintln!("[INF] Socket Status {:?}", status);
                    connection.socket_status = status;
                }

                match status {
                    SocketStatus::CloseWait | SocketStatus::Closed => {
                        return Err(NetworkError::Closed)
                    }
                    SocketStatus::Established => {
                        return Ok(());
                    }
                    _ => {
                        delay.delay_ms(5_u16);
                    }
                }
            }
            None => return Err(NetworkError::SocketStatusNone),
        }
    }
}