This 32 bit static library is built for an arm cortex m3 mcu from linux using a cross compiler. It was too complicated to use windows for this task.
Some minor code changes had to be made to bearssl. Most notably hardcoding the time in the x509_minimal.c file as well as providing in implementation of strlen
which is not available when you don’t link to a standard library. The hardcoded time is a problem which needs to be fixed or else cerificates generated in the future will be rejected.

The static library in this folder is paired with the 32 bit auto generated bearssl.rs file which is built using cargo, rust-bindgen and the windows msvc compiler (the x86 32 bit one).
The documentation and source code for BearSSL can be found at https://bearssl.org

The build.rs file in the folder below this points to this library and links it to the main rust executable for this project. Everything is combined into one binary to be flashed to the device.
