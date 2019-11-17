#![no_std]
#![no_main]

extern crate panic_itm;

#[macro_use]
extern crate cortex_m;

use core::str::Utf8Error;
use cortex_m_rt::entry;
use embedded_hal::digital::v2::OutputPin;
use embedded_hal::{spi::FullDuplex, spi::Mode, spi::Phase, spi::Polarity};
use embedded_websocket as ws;
use max7219::{Command, MAX7219};
use stm32f1xx_hal::{delay::Delay, prelude::*, spi::Spi, stm32};
use w5500::{IpAddress, MacAddress, Socket, SocketStatus, W5500};
use ws::{WebSocketReceiveMessageType, WebSocketSendMessageType, WebSocketServer, WebSocketState};

type SpiFullDuplex = FullDuplex<u8, Error = stm32f1xx_hal::spi::Error>;
use cortex_m::peripheral::itm::Stim;

use core::fmt::Arguments;
use cortex_m::itm;
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
 //   itm::write_str(itm, msg);
 //   itm::write_str(itm, "\n");
}

fn log_fmt(itm: &mut Stim, args: Arguments) {
 //   itm::write_fmt(itm, args);
 //   itm::write_str(itm, "\n");
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
        .map_err(|_e| log(&mut itm.stim[0], "ERROR Unexpected error"));
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
    let root_html = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=UTF-8\r\nContent-Length: 2591\r\nConnection: close\r\n\r\n<!doctype html>
<html>
<head>
    <meta content='text/html;charset=utf-8' http-equiv='Content-Type' />
    <meta content='utf-8' http-equiv='encoding' />
    <meta name='viewport' content='width=device-width, initial-scale=0.5, maximum-scale=0.5, user-scalable=0' />
    <meta name='apple-mobile-web-app-capable' content='yes' />
    <meta name='apple-mobile-web-app-status-bar-style' content='black' />
    <title>Web Socket Demo</title>
    <style type='text/css'>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { font: 13px Helvetica, Arial; }
        form { background: #000; padding: 3px; position: fixed; bottom: 0; width: 100%; }
        form input { border: 0; padding: 10px; width: 90%; margin-right: .5%; }
        form button { width: 9%; background: rgb(130, 200, 255); border: none; padding: 10px; }
        #messages { list-style-type: none; margin: 0; padding: 0; }
        #messages li { padding: 5px 10px; }
        #messages li:nth-child(odd) { background: #eee; }
    </style>
</head>
<body>
    <ul id='messages'></ul>
    <form action=''>
    <input id='txtBox' autocomplete='off' /><button>Send</button>
    </form>
    <script type='text/javascript' src='http://code.jquery.com/jquery-1.11.1.js' ></script>
    <script type='text/javascript'>
        var CONNECTION;
        window.onload = function () {
            // open the connection to the Web Socket server
            // CONNECTION = new WebSocket('ws://' + location.host + ':80/chat');
			CONNECTION = new WebSocket('ws://192.168.1.33:1337/chat');

            // When the connection is open
            CONNECTION.onopen = function () {
                $('#messages').append($('<li>').text('Connection opened'));
            };

            // when the connection is closed by the server
            CONNECTION.onclose = function () {
                $('#messages').append($('<li>').text('Connection closed'));
            };

            // Log errors
            CONNECTION.onerror = function (e) {
                console.log('An error occured');
            };

            // Log messages from the server
            CONNECTION.onmessage = function (e) {
                $('#messages').append($('<li>').text(e.data));
            };
        };

		$(window).on('beforeunload', function(){
			CONNECTION.close();
		});

        // when we press the Send button, send the text to the server
        $('form').submit(function(){
            CONNECTION.send($('#txtBox').val());
            $('#txtBox').val('');
            return false;
        });
    </script>
</body>
</html>";

    let mut w5500 = W5500::new(cs_ethernet);

    w5500.set_mode(spi, false, false, false, false)?;
    w5500.set_mac(spi, &MacAddress::new(0x02, 0x01, 0x02, 0x03, 0x04, 0x05))?;
    w5500.set_ip(spi, &IpAddress::new(192, 168, 1, 33))?;
    w5500.set_subnet(spi, &IpAddress::new(255, 255, 255, 0))?;
    w5500.set_gateway(spi, &IpAddress::new(192, 168, 1, 1))?;

    const PORT: u16 = 1337;

    let mut buffer: [u8; 3000] = [0; 3000];
    let mut ws_buffer: [u8; 500] = [0; 500];

    const NUM_SOCKETS: usize = 8;

    let mut connections: [Connection; NUM_SOCKETS] = [
        Connection::new(Socket::Socket0),
        Connection::new(Socket::Socket1),
        Connection::new(Socket::Socket2),
        Connection::new(Socket::Socket3),
        Connection::new(Socket::Socket4),
        Connection::new(Socket::Socket5),
        Connection::new(Socket::Socket6),
        Connection::new(Socket::Socket7),
    ];

    // make sure all the connections are closed before we start
    for connection in connections.iter() {
        w5500.set_protocol(spi, connection.socket, w5500::Protocol::TCP)?;
        w5500.dissconnect(spi, connection.socket)?;
    }

    loop {
        for index in 0..NUM_SOCKETS {
            let mut connection = &mut connections[index];

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
                            // open
                            log_fmt(
                                itm,
                                format_args!("INFO TCP Opening {:?}", connection.socket),
                            );
                            w5500.open_tcp(spi, connection.socket)?;
                        }
                        SocketStatus::Init => {
                            // listen
                            log_fmt(
                                itm,
                                format_args!(
                                    "INFO TCP Attempting to listen to {:?} on port: {}",
                                    connection.socket, PORT
                                ),
                            );
                            w5500.listen_tcp(spi, connection.socket, PORT)?;
                        }
                        SocketStatus::Established => {
                            eth_read(
                                spi,
                                connection.socket,
                                &mut w5500,
                                &mut connection.web_socket,
                                &mut buffer,
                                &mut ws_buffer,
                                &root_html,
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
    itm: &mut Stim,
    max7219: &mut MAX7219<CS>,
) -> core::result::Result<(), WebServerError>
where
    CS: OutputPin<Error = PinError>,
{
    let ws_read_result = web_socket.read(&eth_buffer, ws_buffer)?;
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
            )?;
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

fn eth_read<CS, PinError>(
    spi: &mut SpiFullDuplex,
    socket: Socket,
    w5500: &mut W5500,
    web_socket: &mut WebSocketServer,
    eth_buffer: &mut [u8],
    ws_buffer: &mut [u8],
    root_html: &str,
    itm: &mut Stim,
    max7219: &mut MAX7219<CS>,
) -> Result<(), WebServerError>
where
    CS: OutputPin<Error = PinError>,
{
    let size = w5500.try_receive_tcp(spi, socket, eth_buffer)?;
    if let Some(size) = size {
        log_fmt(itm, format_args!("INFO Received {} bytes", size));
        if web_socket.state == WebSocketState::Open {
            ws_read(
                spi, socket, w5500, web_socket, eth_buffer, ws_buffer, itm, max7219,
            )?;
        } else {
            let http_header = ws::read_http_header(eth_buffer)?;
            if let Some(websocket_context) = http_header.websocket_context {
                log(itm, "INFO Websocket request. Generating handshake");
                let ws_send = web_socket.server_accept(
                    &websocket_context.sec_websocket_key,
                    None,
                    eth_buffer,
                )?;
                log_fmt(
                    itm,
                    format_args!(
                        "INFO Websocket sending handshake response of {} bytes",
                        ws_send
                    ),
                );
                w5500.send_tcp(spi, socket, &eth_buffer[..ws_send])?;
                log(itm, "INFO Websocket handshake complete");
            } else {
                log_fmt(
                    itm,
                    format_args!("INFO Http File header path: {}", http_header.path),
                );
                match http_header.path.as_str() {
                    "/" => {
                        send_html_and_close(spi, socket, w5500, root_html, itm)?;
                    }
                    _ => {
                        let http = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        send_html_and_close(spi, socket, w5500, http, itm)?;
                    }
                }
            }
        }
    }

    Ok(())
}
