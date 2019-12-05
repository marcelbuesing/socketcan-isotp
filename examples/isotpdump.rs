use socketcan_isotp::{IsoTpBehaviour, IsoTpOptions, IsoTpSocket};

use std::io;

fn main() -> io::Result<()> {
    let mut tp_options = IsoTpOptions::default();
    // tp_options.set_flags(IsoTpBehaviour::CAN_ISOTP_LISTEN_MODE);

    let socket = IsoTpSocket::open("vcan0", 123, 321, None, None, None).unwrap();

    loop {
        let frame = socket.read_frame()?;
        println!("{:#?}", frame);
    }
}
