use core::cell::RefCell;

use crate::{SpiError, SpiTransfer, W5500Physical};
use cortex_m::prelude::_embedded_hal_blocking_delay_DelayMs;
use embedded_websocket::framer::{IoError, Read, Write};
use stm32f1xx_hal::delay::Delay;
use w5500::{IpAddress, MacAddress, Socket, SocketStatus};

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

pub struct TcpStream<'a> {
    w5500: &'a mut W5500Physical,
    connection: Connection,
    delay: &'a RefCell<Delay>,
    spi: &'a RefCell<SpiTransfer>,
}

impl<'a> TcpStream<'a> {
    pub fn new(
        w5500: &'a mut W5500Physical,
        socket: Socket,
        delay: &'a RefCell<Delay>,
        spi: &'a RefCell<SpiTransfer>,
    ) -> Self {
        let connection = Connection::new(socket);
        Self {
            w5500,
            connection,
            delay,
            spi,
        }
    }

    pub fn connect(&mut self, host_ip: &IpAddress, host_port: u16) -> Result<(), NetworkError> {
        rprintln!("[INF] Connecting to {}:{}", host_ip, host_port);

        let spi = &mut *self.spi.borrow_mut();
        let delay = &mut *self.delay.borrow_mut();
        let w5500 = &mut self.w5500;

        w5500.set_mode(spi, false, false, false, false)?;
        w5500.set_mac(spi, &MacAddress::new(0x02, 0x01, 0x02, 0x03, 0x04, 0x05))?;
        w5500.set_subnet(spi, &IpAddress::new(255, 255, 255, 0))?;
        w5500.set_ip(spi, &IpAddress::new(192, 168, 1, 33))?;
        w5500.set_gateway(spi, &IpAddress::new(192, 168, 1, 1))?;
        w5500.set_protocol(spi, self.connection.socket, w5500::Protocol::TCP)?;
        w5500.dissconnect(spi, self.connection.socket)?;
        w5500.open_tcp(spi, self.connection.socket)?;

        w5500.connect(spi, Socket::Socket0, host_ip, host_port)?;
        wait_for_is_connected(w5500, spi, &mut self.connection, delay)?;
        rprintln!("[INF] Client connected");
        Ok(())
    }
}

fn wait_for_is_connected(
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

// TODO: Make Read generic over Error
impl<'a> Read for TcpStream<'a> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        rprintln!("[INF] Read: Waiting for bytes");
        let spi = &mut *self.spi.borrow_mut();
        let delay = &mut *self.delay.borrow_mut();

        loop {
            wait_for_is_connected(&mut self.w5500, spi, &mut self.connection, delay)
                .map_err(|_| IoError::Read)?;
            match self
                .w5500
                .try_receive_tcp(spi, self.connection.socket, buf)
                .map_err(|_| IoError::Read)?
            {
                Some(len) => {
                    rprintln!("[INF] Read: Received {} bytes", len);
                    return Ok(len);
                }
                None => {
                    delay.delay_ms(10_u16);
                }
            };
        }
    }
}

// TODO: Make Write generic over Error
impl<'a> Write for TcpStream<'a> {
    fn write_all(&mut self, buf: &[u8]) -> Result<(), IoError> {
        let mut start = 0;
        rprintln!("[INF] Write: Sending {} bytes", buf.len());
        let spi = &mut *self.spi.borrow_mut();
        let delay = &mut *self.delay.borrow_mut();

        loop {
            wait_for_is_connected(&mut self.w5500, spi, &mut self.connection, delay)
                .map_err(|_| IoError::Write)?;
            let bytes_sent = self
                .w5500
                .send_tcp(spi, self.connection.socket, &buf[start..])
                .map_err(|_| IoError::Write)?;
            start += bytes_sent;
            rprintln!("[INF] Write: Sent {} bytes", bytes_sent);

            if start == buf.len() {
                return Ok(());
            }
        }
    }
}
