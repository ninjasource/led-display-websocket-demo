This project demonstrates a maple mini microcontroller STM32F103C8T6 (20KB Ram and 128KB Flash) connecting to a website using websockets and secured using TLS1.2 with the BearSSL library. 
The w5500 board is used for the ethernet connection and a daisey chain of max7219 chips are used to display data on a scrolling LED panel.
See ./lib/README.txt for instructions on how to build the BearSSL ssl library

The LetsEncrypt trust anchor is used so only sites signed using their root certificate authority will work (up till the year 2035)
At the time of writing the system does not know the current date and time so I have hardcoded some future date and time in its place for now. This allows the x509 certificate date range checks to pass.
Additionally, the system has no way of gathering high quality entropy (used to generate random numbers) so this needs to be addressed too as the crypto is weak as a result. The entropy is currently hardcoded.

Future plans:
1. Use a real time clock to store the date and time or get the time from an online time server
2. Build a hardware random number generator using zener diode reverse breakdown phenomena or use the internal temperature sensor to gather entropy

HOW TO BUILD AND RUN

This project runs on stable rust.
You will need an STLink-V2 to flash the device. 

To build: 
cargo build

To run:
cargo run

TROUBLESHOOING:

Currenly the time is hardcoded which is not ideal Update this to 1 month in the future if you are having trouble with certificate expiry errors