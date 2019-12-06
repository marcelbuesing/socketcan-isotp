use socketcan_isotp::IsoTpSocket;
use std::io;

use socketcan::CANFrame;
fn main() -> io::Result<()> {
    println!("SIZE {}", std::mem::size_of::<CANFrame>());
    let mut tp_socket = IsoTpSocket::open("vcan0", 0x123, 0x321, None, None, None).unwrap();

    let buffer = tp_socket.read()?;
    println!("read {} bytes", buffer.len());

    for x in buffer {
        print!("{:X?} ", x);
    }

    println!("");

    Ok(())
}
