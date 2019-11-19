#![no_std]
#![no_main]

extern crate panic_itm;

#[macro_use]
extern crate cortex_m;

use core::str::Utf8Error;
use cortex_m_rt::entry;
use embedded_hal::digital::v2::OutputPin;
use embedded_hal::{spi::FullDuplex, spi::Mode, spi::Phase, spi::Polarity};
use max7219::{Command, MAX7219};
use stm32f1xx_hal::{delay::Delay, prelude::*, spi::Spi, stm32};
use w5500::{IpAddress, MacAddress, Socket, SocketStatus, W5500};

use embedded_websocket as ws;
use ws::{WebSocketReceiveMessageType, WebSocketSendMessageType, WebSocketServer, WebSocketState, WebSocket, EmptyRng, WebSocketOptions};


type SpiFullDuplex = FullDuplex<u8, Error = stm32f1xx_hal::spi::Error>;
use cortex_m::peripheral::itm::Stim;

use core::fmt::Arguments;
use cortex_m::itm;
use embedded_websocket::WebSocketKey;
//type MAX7219Type<'cs, PinError> = MAX7219<'cs, OutputPin<Error = PinError>>;

#[derive(Debug)]
enum WebServerError {
    Io(stm32f1xx_hal::spi::Error),
    WebSocket(ws::Error),
    Utf8Error,
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
    // TODO: comment these out before demo
  //  itm::write_str(itm, msg);
  //  itm::write_str(itm, "\n");
}

fn log_fmt(itm: &mut Stim, args: Arguments) {
    // TODO: comment these out before demo
  //  itm::write_fmt(itm, args);
  //  itm::write_str(itm, "\n");
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
    log(&mut itm.stim[0], "[INF] Done initializing");

    let mut max7219 = MAX7219::new(&mut cs_max7219, 20);
    max7219.write_command_all(&mut spi, Command::OnOff, 0);
    max7219.write_command_all(&mut spi, Command::ScanLimit, 7);
    max7219.write_command_all(&mut spi, Command::DecodeMode, 0);
    max7219.write_command_all(&mut spi, Command::DisplayTest, 0);
    max7219.clear_all(&mut spi);
    max7219.write_command_all(&mut spi, Command::Intensity, 1);
    max7219.write_command_all(&mut spi, Command::OnOff, 1);

    loop {
        run_web_server(
            &mut spi,
            &mut itm.stim[0],
            &mut delay,
            &mut cs_ethernet,
            &mut max7219,
        )
        .map_err(|e| log_fmt(&mut itm.stim[0], format_args!("ERROR Unexpected error: {:?}", e)));
    }

    log(&mut itm.stim[0], "[WRN] Unexpected end of run loop");
    loop {}
}

fn scroll_str<PinError, CS>(max7219: &mut MAX7219<CS>, spi: &mut SpiFullDuplex, message: &str)
where
    CS: OutputPin<Error = PinError>,
{
    let from_pos = max7219.get_num_devices() * 8;
    let to_pos = message.len() as i32 * -8;
    let mut pos = from_pos as i32;

    loop {
        pos -= 1;

        max7219.write_str_at_pos(spi, message, pos);

        // delay between frames
        //  delay.delay_ms(2_u16);

        // start over
        if pos < to_pos {
            return;
        }
    }
}
fn run_web_server<PinError, CS>(
    spi: &mut SpiFullDuplex,
    itm: &mut Stim,
    delay: &mut Delay,
    cs_ethernet: &mut embedded_hal::digital::OutputPin,
    max7219: &mut MAX7219<CS>,
) -> Result<(), WebServerError>
where
    CS: OutputPin<Error = PinError>,
{
    let mut w5500 = W5500::new(cs_ethernet);

    w5500.set_mode(spi, false, false, false, false)?;
    w5500.set_mac(spi, &MacAddress::new(0x02, 0x01, 0x02, 0x03, 0x04, 0x05))?;
 //  w5500.set_ip(spi, &IpAddress::new(192, 168, 1, 33))?;
 //   w5500.set_subnet(spi, &IpAddress::new(255, 255, 255, 0))?;
 //   w5500.set_gateway(spi, &IpAddress::new(192, 168, 1, 1))?;
    w5500.set_ip(spi, &IpAddress::new(192, 168, 137, 33))?;
    w5500.set_subnet(spi, &IpAddress::new(255, 255, 255, 0))?;
    w5500.set_gateway(spi, &IpAddress::new(192, 168, 137, 1))?;

    const PORT: u16 = 1337;

    let mut buffer: [u8; 3000] = [0; 3000];
    let mut ws_buffer: [u8; 500] = [0; 500];

    const NUM_SOCKETS: usize = 8;

    // make sure the connection is closed before we start
    let mut connection = Connection::new(Socket::Socket0);
    w5500.set_protocol(spi, connection.socket, w5500::Protocol::TCP)?;
    w5500.dissconnect(spi, connection.socket)?;

    let mut buffer: [u8; 3000] = [0; 3000];
    let mut ws_buffer: [u8; 500] = [0; 500];
    let mut web_socket = ws::WebSocket::new_client(EmptyRng::new());
   // let host_ip = IpAddress::new(192,168,1,149);
    let host_ip = IpAddress::new(51,140,68,75);
    // open
    log_fmt(
        itm,
        format_args!("INFO TCP Opening {:?}", connection.socket),
    );
    web_socket = ws::WebSocket::new_client(EmptyRng::new());
    w5500.open_tcp(spi, connection.socket)?;
//    w5500.connect(spi, Socket::Socket0, &host_ip, 1337)?;
    w5500.connect(spi, Socket::Socket0, &host_ip, 80)?;
    loop {
        match w5500.get_socket_status(spi, connection.socket) {
            Ok(Some(socket_status)) => {
                if connection.socket_status != socket_status {
                    // print status change
                    log_fmt(
                        itm,
                        format_args!(
                            "INFO Socket status: {:?} -> {:?}",
                            connection.socket_status, socket_status
                        ),
                    );
                    if socket_status == SocketStatus::Closed {
                        log(itm, "");
                    }
                    connection.socket_status = socket_status;
                }
                match socket_status {
                    SocketStatus::Closed | SocketStatus::CloseWait => {
                        /*
                        // open
                        log_fmt(
                            itm,
                            format_args!("INFO Closed, TCP Opening {:?}", connection.socket),
                        );
                        web_socket = ws::WebSocket::new_client(EmptyRng::new());
                        w5500.open_tcp(spi, connection.socket)?;
                        w5500.connect(spi, Socket::Socket0, &host_ip, 1337)?;*/
                    }
                    SocketStatus::Init => {
                        /*// open
                        log_fmt(
                            itm,
                            format_args!("INFO TCP Opening {:?}", connection.socket),
                        );
                        web_socket = ws::WebSocket::new_client(EmptyRng::new());
                        w5500.open_tcp(spi, connection.socket)?;
                        w5500.connect(spi, Socket::Socket0, &host_ip, 1337)?;*/
                    }
                    SocketStatus::Established => {

                        if web_socket.state == WebSocketState::None {
                            // initiate a websocket opening handshake
                            let websocket_options = WebSocketOptions {
                                path: "/ws/ledpanel",
                                host: "ninjametal.com",
                                origin: "http://ninjametal.com",
                                sub_protocols: None,
                                additional_headers: None,
                            };
                            let (len, web_socket_key) = web_socket.client_connect(&websocket_options, &mut ws_buffer)?;
                            eth_write(spi, Socket::Socket0, &mut w5500, &mut ws_buffer[..len], itm)?;
                        }

                        eth_read_client(
                            spi,
                            Socket::Socket0,
                            &mut w5500,
                            &mut web_socket,
                            &mut buffer,
                            &mut ws_buffer,
                            itm,
                            max7219,
                        )?;
                    }
                    _ => {} // do nothing
                }
            }
            Ok(None) => {
                log(itm, "ERROR Unknown socket status");
                return Ok(());
            }
            Err(_e) => log(itm, "ERROR Cannot read socket status"),
        }
    }
}

fn ws_write_back(
    spi: &mut SpiFullDuplex,
    socket: Socket,
    w5500: &mut W5500,
    web_socket: &mut WebSocketServer,
    eth_buffer: &mut [u8],
    ws_buffer: &mut [u8],
    count: usize,
    send_message_type: WebSocketSendMessageType,
    itm: &mut Stim,
) -> Result<(), WebServerError> {
    eth_buffer[..count].copy_from_slice(&ws_buffer[..count]);
    let ws_to_send = web_socket.write(send_message_type, true, &eth_buffer[..count], ws_buffer)?;
    eth_write(spi, socket, w5500, &ws_buffer[..ws_to_send], itm)?;
    log_fmt(
        itm,
        format_args!(
            "INFO Websocket encoded {:#?}: {} bytes",
            send_message_type, ws_to_send
        ),
    );
    Ok(())
}

fn ws_read<CS, PinError>(
    spi: &mut SpiFullDuplex,
    socket: Socket,
    w5500: &mut W5500,
    web_socket: &mut WebSocketServer,
    eth_buffer: &mut [u8],
    ws_buffer: &mut [u8],
    size : usize,
    itm: &mut Stim,
    max7219: &mut MAX7219<CS>,
) -> core::result::Result<(), WebServerError>
where
    CS: OutputPin<Error = PinError>,
{
    let ws_read_result = web_socket.read(&eth_buffer[..size], ws_buffer)?;
    log_fmt(
        itm,
        format_args!(
            "INFO Websocket decoded {:#?}: {} bytes",
            ws_read_result.message_type, ws_read_result.len_to
        ),
    );
    match ws_read_result.message_type {
        WebSocketReceiveMessageType::Text => {
            {
                let message = ::core::str::from_utf8(&ws_buffer[..ws_read_result.len_to])?;
                log_fmt(itm, format_args!("INFO Websocket: {}", &message));
                scroll_str(max7219, spi, message);
            }

            /*
            ws_write_back(
                spi,
                socket,
                w5500,
                web_socket,
                eth_buffer,
                ws_buffer,
                ws_read_result.len_to,
                WebSocketSendMessageType::Text,
                itm,
            )?;*/
        }
        WebSocketReceiveMessageType::Binary => {
            // do nothing
        }
        WebSocketReceiveMessageType::CloseMustReply => {
            let close_status = ws_read_result.close_status.unwrap(); // this should never fail

            {
                if ws_read_result.len_to > 2 {
                    let message = ::core::str::from_utf8(&ws_buffer[2..ws_read_result.len_to])?;
                    log_fmt(
                        itm,
                        format_args!(
                            "INFO Websocket close status {:#?}: {}",
                            close_status, message
                        ),
                    );
                } else {
                    log_fmt(
                        itm,
                        format_args!("INFO Websocket close status {:#?}", close_status),
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
            log(itm, "INFO TCP connection closed");
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
            log(itm, "INFO Websocket close handshake completed");
            w5500.close(spi, socket)?;
            log(itm, "INFO TCP connection closed");
        }
    }

    Ok(())
}

fn eth_write(
    spi: &mut SpiFullDuplex,
    socket: Socket,
    w5500: &mut W5500,
    buffer: &[u8],
    itm: &mut Stim,
) -> Result<(), WebServerError> {
    let mut start = 0;
    loop {
        let bytes_sent = w5500.send_tcp(spi, socket, &buffer[start..])?;
        log_fmt(itm, format_args!("INFO Sent {} bytes", bytes_sent));
        start += bytes_sent;

        if start == buffer.len() {
            return Ok(());
        }
    }
}

fn send_html_and_close(
    spi: &mut SpiFullDuplex,
    socket: Socket,
    w5500: &mut W5500,
    //eth_buffer: &mut [u8],
    html: &str,
    itm: &mut Stim,
) -> Result<(), WebServerError> {
    log_fmt(itm, format_args!("INFO Sending: {}", html));
    eth_write(spi, socket, w5500, &html.as_bytes(), itm)?;
    w5500.close(spi, socket)?;
    log(itm, "INFO Send complete. Connection closed");
    Ok(())
}

fn eth_read_client<CS, PinError>(
    spi: &mut SpiFullDuplex,
    socket: Socket,
    w5500: &mut W5500,
    web_socket: &mut WebSocket<EmptyRng>,
    eth_buffer: &mut [u8],
    ws_buffer: &mut [u8],
    itm: &mut Stim,
    max7219: &mut MAX7219<CS>,
) -> Result<(), WebServerError>
    where
        CS: OutputPin<Error = PinError>,
{
    let size = w5500.try_receive_tcp(spi, socket, eth_buffer)?;
    if let Some(size) = size {
        log_fmt(itm, format_args!("INFO Received {} bytes", size));

        match web_socket.state {
            WebSocketState::Connecting => {
                let sec_websocket_key = WebSocketKey::new();
                web_socket.client_accept(&sec_websocket_key, &eth_buffer[..size])?;
            },
            WebSocketState::Open => {
                ws_read(
                    spi, socket, w5500, web_socket, eth_buffer, ws_buffer, size, itm, max7219,
                )?;
            },
            _ => log(itm, "Unexpected WebSocketState")
        };
    };

    Ok(())
}
