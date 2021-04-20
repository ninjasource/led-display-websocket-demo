use core::{borrow::BorrowMut, usize};

use cortex_m::peripheral::itm::Stim;
use embedded_hal::{blocking::spi::Transfer, digital::v2::OutputPin};
use embedded_websocket::framer::{IoError, Read, Write};
use shared_bus::{NullMutex, SpiProxy};
use stm32f1xx_hal::{
    gpio::{
        gpioa::{PA2, PA5, PA6, PA7},
        Alternate, Floating, Input, Output, PushPull,
    },
    pac::SPI1,
    spi::Spi,
};
use w5500::{IpAddress, MacAddress, Socket, SocketStatus, W5500};

use crate::{log, log_fmt};

#[derive(Debug)]
pub enum NetworkError {
    Connect,
    Io(stm32f1xx_hal::spi::Error),
}

impl From<stm32f1xx_hal::spi::Error> for NetworkError {
    fn from(err: stm32f1xx_hal::spi::Error) -> NetworkError {
        NetworkError::Io(err)
    }
}

// W5500 ethernet card with CS pin PA2, and the other pins specified too.
// Spi bus shared behind a proxy. Null mutex because this app is single threaded.
// Generics gone mad, lol
pub(crate) type EthernetCard<'a> = W5500<
    'a,
    PA2<Output<PushPull>>,
    SpiProxy<
        'a,
        NullMutex<
            Spi<
                SPI1,
                (
                    PA5<Alternate<PushPull>>,
                    PA6<Input<Floating>>,
                    PA7<Alternate<PushPull>>,
                ),
            >,
        >,
    >,
>;

/*

impl<SpiError, PinError> From<max7219_dot_matrix::Error<SpiError, PinError>> for WebServerError {
    fn from(_err: max7219_dot_matrix::Error<SpiError, PinError>) -> WebServerError {
        // FIXME: capture more of the error than this simple variant
        WebServerError::Max7219
    }
}
*/

struct Connection {
    pub socket: Socket,
    pub socket_status: SocketStatus,
}

impl Connection {
    fn new(socket: Socket) -> Connection {
        Connection {
            socket,
            socket_status: SocketStatus::Closed,
        }
    }
}

pub struct TcpStream<'a, CS, SPI> {
    w5500: &'a mut W5500<'a, CS, SPI>,
    connection: Connection,
    itm: &'a mut Stim,
}

impl<'a, CS, PinError, SPI, SpiError> TcpStream<'a, CS, SPI>
where
    CS: OutputPin<Error = PinError>,
    SPI: Transfer<u8, Error = SpiError>,
    SpiError: core::fmt::Debug,
{
    pub fn new(w5500: &'a mut W5500<'a, CS, SPI>, socket: Socket, itm: &'a mut Stim) -> Self {
        let connection = Connection::new(Socket::Socket0);
        Self {
            w5500,
            connection,
            itm,
        }
    }

    fn wait_for_is_connected(&mut self) -> Result<(), SpiError> {
        loop {
            match self.w5500.get_socket_status(self.connection.socket)? {
                Some(status) => {
                    if status != self.connection.socket_status {
                        log_fmt(self.itm, format_args!("[INF] Socket Status {:?}", status));
                        self.connection.socket_status = status;
                    }

                    match status {
                        SocketStatus::CloseWait | SocketStatus::Closed => {
                            // TODO: return error
                            return Ok(());
                        }
                        SocketStatus::Established => {
                            return Ok(());
                        }
                        _ => {
                            // continue looping
                        }
                    }
                }
                None => {
                    // TODO: error (maybe)
                }
            }
        }
    }

    pub fn connect(&mut self, host_ip: &IpAddress, host_port: u16) -> Result<(), SpiError> {
        self.w5500.set_mode(false, false, false, false)?;
        self.w5500
            .set_mac(&MacAddress::new(0x02, 0x01, 0x02, 0x03, 0x04, 0x05))?;
        self.w5500.set_subnet(&IpAddress::new(255, 255, 255, 0))?;
        self.w5500.set_ip(&IpAddress::new(192, 168, 1, 33))?;
        self.w5500.set_gateway(&IpAddress::new(192, 168, 1, 1))?;
        self.w5500
            .set_protocol(self.connection.socket, w5500::Protocol::TCP)?;
        self.w5500.dissconnect(self.connection.socket)?;
        self.w5500.open_tcp(self.connection.socket)?;

        log_fmt(
            self.itm,
            format_args!("[INF] Connecting to {}:{}", host_ip, host_port),
        );

        self.w5500.connect(Socket::Socket0, host_ip, host_port)?;
        self.wait_for_is_connected()?;
        log(self.itm, "[INF] Client connected");
        Ok(())
    }
}

impl<'a, CS, PinError, SPI, SpiError> Read for TcpStream<'a, CS, SPI>
where
    CS: OutputPin<Error = PinError>,
    SPI: Transfer<u8, Error = SpiError>,
    SpiError: core::fmt::Debug,
{
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        log(self.itm, "[INF] Read: Waiting for bytes");

        loop {
            self.wait_for_is_connected().map_err(|_| IoError::Read)?;
            match self
                .w5500
                .try_receive_tcp(self.connection.socket, buf)
                .map_err(|_| IoError::Read)?
            {
                Some(len) => {
                    log_fmt(self.itm, format_args!("[INF] Read: Received {} bytes", len));
                    return Ok(len);
                }
                None => {
                    //log(self.itm, "[INF] Read: Connected, read 0 bytes");
                    //Ok(0)
                }
            };
        }
    }
}

impl<'a, CS, PinError, SPI, SpiError> Write for TcpStream<'a, CS, SPI>
where
    CS: OutputPin<Error = PinError>,
    SPI: Transfer<u8, Error = SpiError>,
    SpiError: core::fmt::Debug,
{
    fn write_all(&mut self, buf: &[u8]) -> Result<(), IoError> {
        let mut start = 0;
        log_fmt(
            self.itm,
            format_args!("[INF] Write: Sending {} bytes", buf.len()),
        );

        loop {
            self.wait_for_is_connected().map_err(|_| IoError::Read)?;
            let bytes_sent = self
                .w5500
                .send_tcp(self.connection.socket, &buf[start..])
                .map_err(|_| IoError::Write)?;
            start += bytes_sent;
            log_fmt(
                self.itm,
                format_args!("[INF] Write: Sent {} bytes", bytes_sent),
            );

            if start == buf.len() {
                return Ok(());
            }
        }
        Ok(())
    }
}
