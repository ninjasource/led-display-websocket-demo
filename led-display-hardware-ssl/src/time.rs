use crate::{tcp::NetworkError, SpiTransfer, W5500Physical};
use core::convert::TryInto;
use cortex_m::prelude::_embedded_hal_blocking_delay_DelayMs;
use stm32f1xx_hal::delay::Delay;
use w5500::{IpAddress, Socket};

pub static mut UNIX_TIME: crate::bearssl::__time_t = 0;

// NOTE: this should only be called AFTER the w5500 has been correctly set up
pub fn set_time(
    w5500: &mut W5500Physical,
    socket: Socket,
    delay: &mut Delay,
    spi: &mut SpiTransfer,
) -> Result<(), NetworkError> {
    // note that even though the MapleMini has a Real Time Clock built in we cannot rely on it
    // because we do not know if there will be constant power to keep the clock running and
    // the MapleMini Clone is missing the 32768 hz crystal that makes the Rtc tick every second
    // so we don't know how long a tick takes. If you set the frequency to 625hz then you get something
    // close to a second although there is drift.
    // For SSL we only need the time once when making the connection so it is OK to fetch the latest
    // time from an NTP server over the internet every time we attempt to connect

    let mode = 3; // client
    let li = 0; // leap indicator no warning
    const SNTP_VERSION_CONSTANT: u8 = 0x20;
    const NTP_PACKET_LEN: usize = 48;
    let mut request_packet: [u8; NTP_PACKET_LEN] = [0; NTP_PACKET_LEN];
    request_packet[0] = li << 6 | SNTP_VERSION_CONSTANT | mode;

    // this is an IP address of an ntp server found using pool.ntp.org
    let host = IpAddress::new(212, 71, 255, 35);
    const NTP_PORT: u16 = 123;
    rprintln!("[INF] Getting time from NTP server {}:{}", host, NTP_PORT);

    // add a delay here so that we don't spam the NTP server if our chip keeps restarting
    delay.delay_ms(250_u16);

    w5500.set_protocol(spi, socket, w5500::Protocol::UDP)?;
    w5500.send_udp(spi, socket, 0, &host, NTP_PORT, &request_packet)?;

    let mut response_packet: [u8; NTP_PACKET_LEN] = [0; NTP_PACKET_LEN];

    loop {
        match w5500.try_receive_udp(spi, socket, &mut response_packet)? {
            Some((_ip, port, len)) => {
                if port != NTP_PORT {
                    continue;
                }

                if len != NTP_PACKET_LEN {
                    return Err(NetworkError::NtpInvalidPacketLength(len));
                }

                let version = (response_packet[0] >> 3) & 0x07;
                if version != 4 {
                    return Err(NetworkError::NtpInvalidVersion(version));
                }

                let bytes: [u8; 8] = response_packet[32..40].try_into().unwrap();
                let timestamp: u64 = u64::from_be_bytes(bytes);
                let timestamp = if (timestamp & 0x8000_0000_0000_0000) == 0 {
                    timestamp as u128 + 0x0001_0000_0000_0000_0000
                } else {
                    timestamp as u128
                };

                const UNIX_EPOCH: i64 = 2_208_988_800;
                let unix_time_seconds = (timestamp >> 32) as i64 - UNIX_EPOCH;
                rprintln!(
                    "Fetched unix system time from NTP server: {}",
                    unix_time_seconds
                );
                unsafe { UNIX_TIME = unix_time_seconds };

                return Ok(());
            }

            None => {} // continue
        }

        delay.delay_ms(50_u16);
    }
}
