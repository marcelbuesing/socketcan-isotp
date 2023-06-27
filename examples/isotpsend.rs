use socketcan_isotp::{self, IsoTpSocket, StandardId};
use std::time::Duration;

fn main() -> Result<(), socketcan_isotp::Error> {
    let tp_socket = IsoTpSocket::open(
        "vcan0",
        StandardId::new(0x321).expect("Invalid rx id"),
        StandardId::new(0x123).expect("Invalid tx id"),
    )?;

    loop {
        tp_socket.write(&[0xAA, 0x11, 0x22, 0x33, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF])?;
        println!("Sent frame");
        std::thread::sleep(Duration::from_millis(1000));
    }
}
