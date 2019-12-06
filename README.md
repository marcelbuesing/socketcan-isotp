<div align="center">

  <h1>📦✨  socketcan-isotp</h1>

  <p>
    <strong>Socketcan IsoTP Rust crate</strong>
  </p>

  <p>
    <a href="https://github.com/marcelbuesing/socketcan-isotp/actions?query=workflow%3ACI"><img alt="Build Status" src="https://github.com/marcelbuesing/socketcan-isotp/workflows/CI/badge.svg"/></a>
    <a href="https://crates.io/crates/socketcan-isotp"><img alt="crates.io" src="https://meritbadge.herokuapp.com/socketcan-isotp"/></a>
    <a href="https://crates.io/crates/socketcan-isotp"><img alt="crates.io" src="https://img.shields.io/crates/l/socketcan-isotp/0.1.0"/></a>
  </p>

  <h3>
    <a href="https://docs.rs/socketcan-isotp">Docs</a>
  </h3>

  <sub>Built with 🦀</sub>
</div>

SocketCAN ISO-TP crate. Based on socketcan-rs and isotp.h.

The Linux kernel supports using CAN-devices through a
[network-like API](https://www.kernel.org/doc/Documentation/networking/can.txt).
This crate allows easy access to this functionality without having to wrestle
libc calls.

ISO-TP allows sending data packets that exceed the eight byte of a default CAN frame.
[can-isotp](https://github.com/hartkopp/can-isotp) is an ISO-TP kernel module that takes
care of handling the ISO-TP protocol.

Instructions on how the can-isotp kernel module can be build and loaded can be found
at [https://github.com/hartkopp/can-isotp](https://github.com/hartkopp/can-isotp) .

```rust,no_run
use socketcan_isotp::{self, IsoTpSocket};

fn main() -> Result<(), socketcan_isotp::Error> {
    let mut tp_socket = IsoTpSocket::open(
        "vcan0",
        0x123,
        0x321,
        None,
        None,
        None,
    )?;

    loop {
        let buffer = tp_socket.read()?;
        println!("read {} bytes", buffer.len());

        // print TP frame data
        for x in buffer {
            print!("{:X?} ", x);
        }
        println!("");
    }

    Ok(())
}
```

# Dev Setup

Setup Isotp Kernel Module:
https://github.com/hartkopp/can-isotp

Setup virtual can interface.
```
sudo modprobe vcan && \
sudo ip link add dev vcan0 type vcan && \
sudo ip link set up vcan0
```
