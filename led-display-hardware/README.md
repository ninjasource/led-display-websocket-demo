# Introduction
This demo uses a STM32 Bluepill connected to a W5500 ethernet card and a set of 20 daisy chained MAX7219 boards for use as an LED Display. On startup the application opens a TCP connection to a server over port 80 followed by a websocket opening handshake. It then captures text messages from the websocket connection and scrolls them on the LED Display. The W5500 card has its own internal buffers so we don't have to worry about not being able to read bytes off the network stream immediately.

# Setup

You will need [`probe-run`](https://ferrous-systems.com/blog/probe-run/) - a utility to enable `cargo run` to run embedded applications on a device. The `bluepill` or `maple mini` (STM32F103C8T6) can be programmed with an STLink-V2 USB device.


```
cargo install probe-run
```
We also add a target for arm cross compilation.
```
rustup target add thumbv7m-none-eabi
```

If your dev env is VS Code and you are using rust-analyzer then the following `settings.json` file tells rust-analyzer that this is a `no_std` priject:

```
{
    "rust-analyzer.checkOnSave.allTargets": false,
    "rust-analyzer.checkOnSave.extraArgs": [
        "--target",
        "thumbv7m-none-eabi",
    ]
}
```


# To Run

```cargo run```

