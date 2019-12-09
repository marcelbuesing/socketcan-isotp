use socketcan_isotp::{self, IsoTpSocket};

fn main() -> Result<(), socketcan_isotp::Error> {
    let mut tp_socket = IsoTpSocket::open("vcan0", 0x123, 0x321)?;

    let buffer = tp_socket.read()?;
    println!("read {} bytes", buffer.len());

    for x in buffer {
        print!("{:X?} ", x);
    }

    println!("");

    Ok(())
}
