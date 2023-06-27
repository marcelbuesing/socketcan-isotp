use socketcan_isotp::{self, IsoTpSocket, StandardId};

fn main() -> Result<(), socketcan_isotp::Error> {
    let mut tp_socket = IsoTpSocket::open(
        "vcan0",
        StandardId::new(0x123).expect("Invalid rx id"),
        StandardId::new(0x321).expect("Invalid tx id"),
    )?;

    let buffer = tp_socket.read()?;
    println!("read {} bytes", buffer.len());

    for x in buffer {
        print!("{:X?} ", x);
    }

    println!("");

    Ok(())
}
