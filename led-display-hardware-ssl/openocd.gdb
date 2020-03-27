target extended-remote :3333

# Disable all messages.
set verbose off
set complaints 0
set confirm off
set exec-done-display off
show exec-done-display
set trace-commands off
set debug displaced off 
set debug expression 0
set debug frame 0
set debug infrun 0
set debug observer 0
set debug overload 0
set pagination off
set print address off
set print symbol-filename off
set print symbol off
set print pretty off
set print object off
set debug parser off
set debug remote 0

# print demangled symbols
set print asm-demangle on

# detect unhandled exceptions, hard faults and panics
# break DefaultHandler
# break UserHardFault
# break rust_begin_unwind

# *try* to stop at the user entry point (it might be gone due to inlining)
break main

monitor arm semihosting enable

# # send captured ITM to the file itm.fifo
# # (the microcontroller SWO pin must be connected to the programmer SWO pin)
# # 8000000 must match the core clock frequency
# monitor tpiu config internal itm.txt uart off 8000000

# # OR: make the microcontroller SWO pin output compatible with UART (8N1)
# # 8000000 must match the core clock frequency
# # 2000000 is the frequency of the SWO pin
# NOTE: This is only used when you run openocd from the command line. If you use the vs code cortex debug plugin this is all defined in the launch.json file
monitor tpiu config external uart off 8000000 1000000

# # enable ITM port 0
# NOTE: This is only used when you run openocd from the command line. If you use the vs code cortex debug plugin this is all defined in the launch.json file

monitor itm port 0 on

load

# start the process but immediately halt the processor
# stepi

continue
continue