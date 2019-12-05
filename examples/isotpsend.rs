use socketcan_isotp::{IsoTpBehaviour, IsoTpOptions, IsoTpSocket};

use std::io;

fn main() -> io::Result<()> {
    let mut tp_options = IsoTpOptions::default();
    // tp_options.set_flags(IsoTpBehaviour::CAN_ISOTP_LISTEN_MODE);

    let socket = IsoTpSocket::open("vcan0", 123, 321, None, None, None).unwrap();

    loop {
        socket.write(&[0x00, 0x11, 0x22, 0x33, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF])?;
        println!("Sent frame");
        std::thread::sleep_ms(1000);
    }
}
