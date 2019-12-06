use socketcan_isotp::{IsoTpBehaviour, IsoTpOptions, IsoTpSocket};

use std::io;

fn main() -> io::Result<()> {
    let tp_socket = IsoTpSocket::open("vcan0", 0x321, 0x123, None, None, None).unwrap();

    loop {
        tp_socket.write(&[0xAA, 0x11, 0x22, 0x33, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF])?;
        println!("Sent frame");
        std::thread::sleep_ms(1000);
    }
}
