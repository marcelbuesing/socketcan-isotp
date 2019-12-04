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

Based on socketcan-rs and isotp.h.

# Dev Setup

Setup Isotp Kernel Module:
https://github.com/hartkopp/can-isotp

Setup virtual can interface.
```
sudo modprobe vcan && \
sudo ip link add dev vcan0 type vcan && \
sudo ip link set up vcan0
```
