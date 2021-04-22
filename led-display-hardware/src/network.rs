use core::cell::RefCell;

use cortex_m::prelude::_embedded_hal_blocking_delay_DelayMs;
use embedded_hal::{blocking::spi::Transfer, digital::v2::OutputPin};
use embedded_websocket::framer::{IoError, Read, Write};
use shared_bus::{NullMutex, SpiProxy};
use stm32f1xx_hal::{
    delay::Delay,
    gpio::{
        gpioa::{PA2, PA5, PA6, PA7},
        Alternate, Floating, Input, Output, PushPull,
    },
    pac::SPI1,
    spi::{Spi, Spi1NoRemap},
};
use w5500::{IpAddress, MacAddress, Socket, SocketStatus, W5500};

use crate::SpiError;

#[derive(Debug)]
pub enum NetworkError {
    Io(stm32f1xx_hal::spi::Error),
    Closed,
    SocketStatusNone,
}

impl From<SpiError> for NetworkError {
    fn from(err: SpiError) -> NetworkError {
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
                Spi1NoRemap,
                (
                    PA5<Alternate<PushPull>>,
                    PA6<Input<Floating>>,
                    PA7<Alternate<PushPull>>,
                ),
                u8,
            >,
        >,
    >,
>;

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
    delay: &'a RefCell<Delay>,
}

impl<'a, CS, PinError, SPI> TcpStream<'a, CS, SPI>
where
    CS: OutputPin<Error = PinError>,
    SPI: Transfer<u8, Error = SpiError>,
{
    pub fn new(
        w5500: &'a mut W5500<'a, CS, SPI>,
        socket: Socket,
        delay: &'a RefCell<Delay>,
    ) -> Self {
        let connection = Connection::new(socket);
        Self {
            w5500,
            connection,
            delay,
        }
    }

    fn wait_for_is_connected(&mut self) -> Result<(), NetworkError> {
        loop {
            match self.w5500.get_socket_status(self.connection.socket)? {
                Some(status) => {
                    if status != self.connection.socket_status {
                        rprintln!("[INF] Socket Status {:?}", status);
                        self.connection.socket_status = status;
                    }

                    match status {
                        SocketStatus::CloseWait | SocketStatus::Closed => {
                            return Err(NetworkError::Closed)
                        }
                        SocketStatus::Established => {
                            return Ok(());
                        }
                        _ => {
                            self.delay.borrow_mut().delay_ms(5_u16);
                        }
                    }
                }
                None => return Err(NetworkError::SocketStatusNone),
            }
        }
    }

    pub fn connect(&mut self, host_ip: &IpAddress, host_port: u16) -> Result<(), NetworkError> {
        rprintln!("[INF] Connecting to {}:{}", host_ip, host_port);
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

        self.w5500.connect(Socket::Socket0, host_ip, host_port)?;
        self.wait_for_is_connected()?;
        rprintln!("[INF] Client connected");
        Ok(())
    }
}

impl<'a, CS, PinError, SPI> Read for TcpStream<'a, CS, SPI>
where
    CS: OutputPin<Error = PinError>,
    SPI: Transfer<u8, Error = SpiError>,
{
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        rprintln!("[INF] Read: Waiting for bytes");

        loop {
            self.wait_for_is_connected().map_err(|_| IoError::Read)?;
            match self
                .w5500
                .try_receive_tcp(self.connection.socket, buf)
                .map_err(|_| IoError::Read)?
            {
                Some(len) => {
                    rprintln!("[INF] Read: Received {} bytes", len);
                    return Ok(len);
                }
                None => {
                    self.delay.borrow_mut().delay_ms(10_u16);
                }
            };
        }
    }
}

impl<'a, CS, PinError, SPI> Write for TcpStream<'a, CS, SPI>
where
    CS: OutputPin<Error = PinError>,
    SPI: Transfer<u8, Error = SpiError>,
{
    fn write_all(&mut self, buf: &[u8]) -> Result<(), IoError> {
        let mut start = 0;
        rprintln!("[INF] Write: Sending {} bytes", buf.len());

        loop {
            self.wait_for_is_connected().map_err(|_| IoError::Read)?;
            let bytes_sent = self
                .w5500
                .send_tcp(self.connection.socket, &buf[start..])
                .map_err(|_| IoError::Write)?;
            start += bytes_sent;
            rprintln!("[INF] Write: Sent {} bytes", bytes_sent);

            if start == buf.len() {
                return Ok(());
            }
        }
    }
}
