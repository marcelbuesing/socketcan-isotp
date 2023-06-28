# Change Log

## [1.0.2]
- Rename src and dst to rx_id and tx_id to avoid confusion
- Bump nix dependency to `0.26`
- Bump bitflags dependency to `2.3`
- Bump embedded-can dependency to `0.4`

## [1.0.1]
- Add public `FlowControlOptions::new`. Thanks Ashcon Mohseninia.
- Bump nix dependency to `0.24`

## [1.0.0]
- Breaking: src and dst identifier are now based on `embedded_can::Id`.
- Bump nix dependency to `0.22`

## [0.1.1](https://github.com/marcelbuesing/socketcan-isotp/tree/1.0.1) (2020-02-18)
- Critical FIX: Source and destination CAN identifiers were mixed up
