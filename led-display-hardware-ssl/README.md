This project demonstrates a maple mini microcontroller STM32F103C8T6 (20KB Ram and 128KB Flash) connecting to a website using websockets and secured using TLS1.2 with the BearSSL library. 
The w5500 board is used for the ethernet connection and a daisey chain of max7219 chips are used to display data on a scrolling LED panel.
See ./lib/README.txt for instructions on how to build the BearSSL ssl library

The LetsEncrypt trust anchors (both the old and the new one) are used so only sites signed using their root certificate authorities will work (up till the year 2035)
Currently, the system has no way of gathering high quality entropy (used to generate random numbers) so this needs to be addressed too as the crypto is weak as a result. The entropy is currently hardcoded.

The system time is fetched from an NTP server on the internet.

Future plans:
Use the internal temperature sensor to gather entropy so that we don't have to hard code it.

HOW TO BUILD AND RUN

This project runs on stable rust.
You will need an STLink-V2 to flash the device. 

To build: 
cargo build

To run:
cargo run

TROUBLESHOOING:

If you want to troubleshoot the network traffic you can set the gateway to a machine on your local network
and point the w5500 card to that gateway. You and then run a packet sniffer like wireshark.
