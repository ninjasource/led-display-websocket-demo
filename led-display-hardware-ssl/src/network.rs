use crate::bearssl::*;
use crate::{SpiError, SpiPhysical, SpiTransfer};
use core::{cell::RefCell, convert::Infallible};
use core::{fmt::Arguments, mem::MaybeUninit, pin::Pin};
use cortex_m::asm;
use cortex_m::prelude::_embedded_hal_blocking_delay_DelayMs;
use cty::size_t;
use embedded_websocket::framer::Stream;
use stm32f1xx_hal::{
    delay::Delay,
    gpio::{gpioa::PA2, Output, PushPull},
};
use w5500::{IpAddress, MacAddress, Socket, SocketStatus, W5500};

// ************ SSL Related ********************

#[derive(Debug)]
enum SslError {
    WriteBrErr(i32),
    ReadBrErr(i32),
}

impl<'a> Stream<NetworkError> for SslStream<'a> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, NetworkError> {
        rprintln!("[INF] read");

        let rlen =
            unsafe { br_sslio_read(self.ssl.ioc, buf as *mut _ as *mut cty::c_void, buf.len()) };

        if rlen < 0 {
            rprintln!("[ERR] br_sslio_read failed to read: {}", rlen);
            return Err(NetworkError::Closed);
            //  return Err(SslError::ReadBrErr(self.ssl.cc.eng.err));
        }

        Ok(rlen as usize)
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<(), NetworkError> {
        rprintln!("[INF] write_all: {} bytes", buf.len());

        let success =
            unsafe { br_sslio_write_all(self.ssl.ioc, buf.as_ptr() as *const _, buf.len()) };

        if success < 0 {
            rprintln!("[ERR] br_sslio_write_all failed: {}", success);

            //return Err(SslError::WriteBrErr(self.ssl.cc.eng.err));
            return Err(NetworkError::Closed);
        }

        rprintln!("[INF] br_sslio_flush");
        unsafe { br_sslio_flush(self.ssl.ioc) };
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
    pub spi: *mut cty::c_void,
    pub connection: *mut cty::c_void,
    pub delay: *mut cty::c_void,
    pub w5500: *mut cty::c_void,
}

pub struct Ssl {
    pub cc: *mut br_ssl_client_context,
    pub ioc: *mut br_sslio_context,
}

pub struct SslStream<'a> {
    w5500: &'a mut W5500Physical,
    connection: Connection,
    spi: &'a mut SpiTransfer,
    ssl: Ssl,
}

impl<'a> SslStream<'a> {
    pub fn new(
        w5500: &'a mut W5500Physical,
        connection: Connection,
        spi: &'a mut SpiTransfer,
        ssl: Ssl,
    ) -> Self {
        Self {
            w5500,
            connection,
            spi,
            ssl,
        }
    }

    pub fn connect(&mut self, host_ip: &IpAddress, host_port: u16) -> Result<(), NetworkError> {
        //    rprintln!("[INF] Connecting to {}:{}", host_ip, host_port);

        loop {
            //      rprintln!("echo");
            self.delay_ms(1000_u16);
            break;
        }

        let w5500 = &mut self.w5500;

        w5500.set_mode(self.spi, false, false, false, false)?;
        w5500.set_mac(
            self.spi,
            &MacAddress::new(0x02, 0x01, 0x02, 0x03, 0x04, 0x05),
        )?;
        w5500.set_subnet(self.spi, &IpAddress::new(255, 255, 255, 0))?;
        w5500.set_ip(self.spi, &IpAddress::new(192, 168, 1, 33))?;
        w5500.set_gateway(self.spi, &IpAddress::new(192, 168, 1, 1))?;
        w5500.set_protocol(self.spi, self.connection.socket, w5500::Protocol::TCP)?;
        w5500.dissconnect(self.spi, self.connection.socket)?;
        w5500.open_tcp(self.spi, self.connection.socket)?;
        w5500.connect(self.spi, self.connection.socket, host_ip, host_port)?;

        return Ok(());
        wait_for_is_connected(w5500, self.spi, &mut self.connection)?;
        rprintln!("[INF] Client connected");

        Ok(())
    }

    pub fn delay_ms(&mut self, ms: u16) {
        //   self.delay.delay_ms(ms)
    }
}

pub fn wait_for_is_connected(
    w5500: &mut W5500Physical,
    spi: &mut SpiTransfer,
    connection: &mut Connection,
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
                        // delay.delay_ms(5_u16);
                    }
                }
            }
            None => return Err(NetworkError::SocketStatusNone),
        }
    }
}
/*
impl<'a> Stream<NetworkError> for TcpStream<'a> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, NetworkError> {
        rprintln!("[INF] Read: Waiting for bytes");
        let spi = &mut *self.spi.borrow_mut();

        loop {
            wait_for_is_connected(&mut self.w5500, spi, &mut self.connection, self.delay)?;
            match self
                .w5500
                .try_receive_tcp(spi, self.connection.socket, buf)?
            {
                Some(len) => {
                    rprintln!("[INF] Read: Received {} bytes", len);
                    return Ok(len);
                }
                None => {
                    self.delay.delay_ms(10_u16);
                }
            };
        }
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<(), NetworkError> {
        let mut start = 0;
        rprintln!("[INF] Write: Sending {} bytes", buf.len());
        let spi = &mut *self.spi.borrow_mut();

        loop {
            wait_for_is_connected(&mut self.w5500, spi, &mut self.connection, self.delay)?;
            let bytes_sent = self
                .w5500
                .send_tcp(spi, self.connection.socket, &buf[start..])?;
            start += bytes_sent;
            rprintln!("[INF] Write: Sent {} bytes", bytes_sent);

            if start == buf.len() {
                return Ok(());
            }
        }
    }
}
*/
