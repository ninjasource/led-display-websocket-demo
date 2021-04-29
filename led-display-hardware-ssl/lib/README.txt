This project uses BearSSL as a 32 bit static library is built for an arm cortex m3 mcu from linux using a cross compiler. It was too complicated to use windows for this task so the build instructions below are for Linux.
The static library is paired with the 32 bit auto generated bearssl.rs file which is built using cargo, rust-bindgen and the windows msvc compiler (the x86 32 bit one).

The documentation and source code for BearSSL can be found at https://bearssl.org
The build.rs file in the folder below this points to this library and links it to the main rust executable for this project. Everything is combined into one binary to be flashed to the device.

HOW TO BUILD libbearssl.a
The following instructions explain how to build BearSSL in Linux using arm-none-eabi-gcc which an arm cross compiler compatible with gcc.
Assuming that you already have the arm gcc toolchain setup on linux run the following in your terminal:

$ git clone https://www.bearssl.org/git/BearSSL
$ cd BearSSL
<now copy cortexm3.mk into the ./conf folder alogside the other make files>
$ make CONF=cortexm3
<this will generate a statically linked library file libbearssl.a in the ./build folder>
<copy libbearssl.a to your rust project root (same folder as build.rs)>
