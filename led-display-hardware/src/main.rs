#![no_std]
#![no_main]

extern crate panic_itm;

use core::str::Utf8Error;
use cortex_m_rt::entry;
use embedded_hal::digital::v2::OutputPin;
use embedded_hal::{spi::FullDuplex, spi::Mode, spi::Phase, spi::Polarity};
use max7219_dot_matrix::{Command, MAX7219};
use stm32f1xx_hal::{delay::Delay, prelude::*, spi::Spi, stm32};
use w5500::{IpAddress, MacAddress, Socket, SocketStatus, W5500};

use embedded_websocket as ws;
use ws::{
    EmptyRng, WebSocket, WebSocketOptions, WebSocketReceiveMessageType, WebSocketSendMessageType,
    WebSocketServer, WebSocketState,
};

type SpiFullDuplex = dyn FullDuplex<u8, Error = stm32f1xx_hal::spi::Error>;
use cortex_m::peripheral::itm::Stim;

use core::fmt::Arguments;
use cortex_m::itm;
use embedded_websocket::WebSocketKey;

#[derive(Debug)]
enum WebServerError {
    Io(stm32f1xx_hal::spi::Error),
    WebSocket(ws::Error),
    Utf8Error,
    Max7219,
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
    pub web_socket: WebSocketServer,
    pub socket: Socket,
    pub socket_status: SocketStatus,
}

impl Connection {
    fn new(socket: Socket) -> Connection {
        Connection {
            web_socket: WebSocketServer::new_server(),
            socket,
            socket_status: SocketStatus::Closed,
        }
    }
}

fn log(itm: &mut Stim, msg: &str) {
    // FIXME: comment these out before demo - itm is not setup correctly without openocd running
    itm::write_str(itm, msg);
    itm::write_str(itm, "\n");
}

fn log_fmt(itm: &mut Stim, args: Arguments) {
    // FIXME: comment these out before demo - itm is not setup correctly without openocd running
    itm::write_fmt(itm, args);
    itm::write_str(itm, "\n");
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
    let mut max7219 = MAX7219::new(&mut cs_max7219, 20);
    let mut w5500 = W5500::new(&mut cs_ethernet);


    run_loop(&mut spi, &mut itm.stim[0], &mut max7219, &mut w5500)
        .map_err(|e| {
            log_fmt(
                &mut itm.stim[0],
                format_args!("[ERR] Unexpected error: {:?}", e),
            )
        })
        .unwrap();

    loop {}
}

fn run_loop<PinError, CS1, CS2>(
    spi: &mut SpiFullDuplex,
    itm: &mut Stim,
    max7219: &mut MAX7219<CS1>,
    w5500: &mut W5500<CS2>,
) -> Result<(), WebServerError>
where
    CS1: OutputPin<Error = PinError>,
    CS2: OutputPin<Error = PinError>,
{
    log(itm, "[INF] Done initializing");

    max7219.write_command_all(spi, Command::OnOff, 0)?;
    max7219.write_command_all(spi, Command::ScanLimit, 7)?;
    max7219.write_command_all(spi, Command::DecodeMode, 0)?;
    max7219.write_command_all(spi, Command::DisplayTest, 0)?;
    max7219.clear_all(spi)?;
    max7219.write_command_all(spi, Command::Intensity, 1)?;
    max7219.write_command_all(spi, Command::OnOff, 1)?;

    loop {
        client_connect(spi, itm, max7219, w5500)
            .map_err(|e| log_fmt(itm, format_args!("[ERR] Unexpected error: {:?}", e)))
            .unwrap();
    }
}

fn scroll_str<PinError, CS>(
    itm: &mut Stim,
    max7219: &mut MAX7219<CS>,
    spi: &mut SpiFullDuplex,
    message: &str,
) -> Result<(), WebServerError>
where
    CS: OutputPin<Error = PinError>,
{
    log_fmt(
        itm,
        format_args!(
            "[DBG] Scrolling message with {:?} characters",
            message.len()
        ),
    );
    let from_pos = max7219.get_num_devices() * 8;
    let to_pos = message.len() as i32 * -8;
    let mut pos = from_pos as i32;

    loop {
        pos -= 1;

        max7219.write_str_at_pos(spi, message, pos)?;

        // start over
        if pos < to_pos {
            log(itm, "[DBG] Done scrolling message");
            return Ok(());
        }
    }
}

fn client_connect<PinError, CS1, CS2>(
    spi: &mut SpiFullDuplex,
    itm: &mut Stim,
    max7219: &mut MAX7219<CS1>,
    w5500: &mut W5500<CS2>,
) -> Result<(), WebServerError>
where
    CS1: OutputPin<Error = PinError>,
    CS2: OutputPin<Error = PinError>,
{
    w5500.set_mode(spi, false, false, false, false)?;
    w5500.set_mac(spi, &MacAddress::new(0x02, 0x01, 0x02, 0x03, 0x04, 0x05))?;
    w5500.set_subnet(spi, &IpAddress::new(255, 255, 255, 0))?;
    //    w5500.set_ip(spi, &IpAddress::new(192, 168, 137, 33))?;
    //    w5500.set_gateway(spi, &IpAddress::new(192, 168, 137, 1))?;
    w5500.set_ip(spi, &IpAddress::new(192, 168, 1, 33))?;
    w5500.set_gateway(spi, &IpAddress::new(192, 168, 1, 1))?;

    // make sure the connection is closed before we start
    let mut connection = Connection::new(Socket::Socket0);
    w5500.set_protocol(spi, connection.socket, w5500::Protocol::TCP)?;
    w5500.dissconnect(spi, connection.socket)?;

    let mut buffer: [u8; 3000] = [0; 3000];
    let mut ws_buffer: [u8; 500] = [0; 500];
    //    let host_ip = IpAddress::new(51, 140, 68, 75);
    //    let host_port = 80;
    //    let host = "ninjametal.com";
    //    let origin = "http://ninjametal.com";

    let host_ip = IpAddress::new(192, 168, 1, 149);
    let host_port = 1337;
    let host = "192.168.1.149";
    let origin = "http://192.168.1.149";

    // open
    log_fmt(
        itm,
        format_args!("[INF] TCP Opening {:?}", connection.socket),
    );
    let mut web_socket = ws::WebSocket::new_client(EmptyRng::new());
    w5500.open_tcp(spi, connection.socket)?;
    w5500.connect(spi, Socket::Socket0, &host_ip, host_port)?;
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
                        web_socket = ws::WebSocket::new_client(EmptyRng::new());
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
                            eth_write(spi, Socket::Socket0, w5500, &ws_buffer[..len], itm)?;
                        }

                        eth_read_client(
                            spi,
                            Socket::Socket0,
                            &mut web_socket,
                            &mut buffer,
                            &mut ws_buffer,
                            itm,
                            max7219,
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

fn ws_write_back<PinError, CS>(
    spi: &mut SpiFullDuplex,
    socket: Socket,
    w5500: &mut W5500<CS>,
    web_socket: &mut WebSocketServer,
    eth_buffer: &mut [u8],
    ws_buffer: &mut [u8],
    count: usize,
    send_message_type: WebSocketSendMessageType,
    itm: &mut Stim,
) -> Result<(), WebServerError>
where
    CS: OutputPin<Error = PinError>
{
    eth_buffer[..count].copy_from_slice(&ws_buffer[..count]);
    let ws_to_send = web_socket.write(send_message_type, true, &eth_buffer[..count], ws_buffer)?;
    eth_write(spi, socket, w5500, &ws_buffer[..ws_to_send], itm)?;
    log_fmt(
        itm,
        format_args!(
            "[DBG] Websocket encoded {:#?}: {} bytes",
            send_message_type, ws_to_send
        ),
    );
    Ok(())
}

fn ws_read<PinError, CS1, CS2>(
    spi: &mut SpiFullDuplex,
    socket: Socket,
    web_socket: &mut WebSocketServer,
    eth_buffer: &mut [u8],
    ws_buffer: &mut [u8],
    size: usize,
    itm: &mut Stim,
    max7219: &mut MAX7219<CS1>,
    w5500: &mut W5500<CS2>,
) -> core::result::Result<(), WebServerError>
where
    CS1: OutputPin<Error = PinError>,
    CS2: OutputPin<Error = PinError>,
{
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
            scroll_str(itm, max7219, spi, message)?;
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
                spi,
                socket,
                w5500,
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
                spi,
                socket,
                w5500,
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

fn eth_write<PinError, CS>(
    spi: &mut SpiFullDuplex,
    socket: Socket,
    w5500: &mut W5500<CS>,
    buffer: &[u8],
    itm: &mut Stim,
) -> Result<(), WebServerError>
where
    CS: OutputPin<Error = PinError>,
{
    let mut start = 0;
    loop {
        let bytes_sent = w5500.send_tcp(spi, socket, &buffer[start..])?;
        log_fmt(
            itm,
            format_args!("[DBG] Ethernet sent {} bytes", bytes_sent),
        );
        start += bytes_sent;

        if start == buffer.len() {
            return Ok(());
        }
    }
}

fn eth_read_client<PinError, CS1, CS2>(
    spi: &mut SpiFullDuplex,
    socket: Socket,
    web_socket: &mut WebSocket<EmptyRng>,
    eth_buffer: &mut [u8],
    ws_buffer: &mut [u8],
    itm: &mut Stim,
    max7219: &mut MAX7219<CS1>,
    w5500: &mut W5500<CS2>,
) -> Result<(), WebServerError>
where
    CS1: OutputPin<Error = PinError>,
    CS2: OutputPin<Error = PinError>,
{
    let size = w5500.try_receive_tcp(spi, socket, eth_buffer)?;
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
                    spi, socket, web_socket, eth_buffer, ws_buffer, size, itm, max7219, w5500,
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
